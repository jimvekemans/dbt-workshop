//! Shared CLI runtime: logging initialization, streaming NDJSON run-log capture, and exit-code mapping.

use std::fmt::{Debug, Write as _};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context as LayerContext, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer, Registry};
use vdp_core::Outcome;

use crate::args::GlobalArgs;

/// Single captured log event serialized as one NDJSON line (`--log-json` / `$VDP_LOG_JSON`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRecord {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// RAII guard that flushes the NDJSON run-log [`BufWriter`] on drop without changing exit codes or masking errors.
#[derive(Debug)]
pub struct RunLogGuard {
    program: String,
    path: PathBuf,
    writer: Arc<Mutex<BufWriter<File>>>,
}

impl Drop for RunLogGuard {
    fn drop(&mut self) {
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(err) = writer.flush() {
            eprintln!(
                "{}: warning: could not flush JSON run log {}: {err}",
                self.program,
                self.path.display()
            );
        }
    }
}

struct JsonCaptureLayer {
    program: String,
    path: PathBuf,
    writer: Arc<Mutex<BufWriter<File>>>,
    warned: AtomicBool,
}

impl JsonCaptureLayer {
    fn write_record(&self, record: &LogRecord) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        serde_json::to_writer(&mut *writer, record).map_err(io::Error::other)?;
        writer.write_all(b"\n")?;
        writer.flush()
    }
}

impl<S: Subscriber> Layer<S> for JsonCaptureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: LayerContext<'_, S>) {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        let meta = event.metadata();
        let record = LogRecord {
            timestamp: format_utc_rfc3339(SystemTime::now()),
            level: meta.level().as_str().to_owned(),
            target: meta.target().to_owned(),
            message: visitor.finish(),
        };
        if let Err(err) = self.write_record(&record)
            && !self.warned.swap(true, Ordering::Relaxed)
        {
            eprintln!(
                "{}: warning: could not write JSON run log {}: {err}",
                self.program,
                self.path.display()
            );
        }
    }
}

#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl EventVisitor {
    fn finish(self) -> String {
        match (self.message, self.fields.is_empty()) {
            (Some(msg), true) => msg,
            (Some(msg), false) => format!("{msg} ({})", self.fields.join(", ")),
            (None, _) => self.fields.join(", "),
        }
    }
}

impl Visit for EventVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_owned());
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }
}

fn open_ndjson_writer(path: &Path) -> io::Result<BufWriter<File>> {
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    Ok(BufWriter::new(file))
}

fn format_utc_rfc3339(time: SystemTime) -> String {
    format_utc_duration(time.duration_since(UNIX_EPOCH).unwrap_or_default())
}

fn format_utc_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let millis = duration.subsec_millis();
    let days = i64::try_from(total_secs / 86_400).unwrap_or(0);
    let secs_of_day = total_secs % 86_400;
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    let mut out = String::with_capacity(24);
    let _ = write!(
        out,
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
    );
    out
}

/// Converts days since `1970-01-01` into `(year, month, day)` in the proleptic Gregorian calendar.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = u32::try_from(z.rem_euclid(146_097)).unwrap_or(0);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year_base = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (year_base + i64::from(month <= 2), month, day)
}

fn env_filter(default_filter: &str) -> EnvFilter {
    EnvFilter::try_from_env("VDP_LOG").unwrap_or_else(|_| default_filter.into())
}

fn stderr_env_filter(default_filter: &str, is_wrapper: bool) -> EnvFilter {
    let filter = env_filter(default_filter);
    if is_wrapper {
        filter.add_directive(
            format!("{}=off", vdp_core::milestone::TARGET)
                .parse()
                .expect("valid milestone filter directive"),
        )
    } else {
        filter
    }
}

fn stderr_layer() -> impl Layer<Registry> {
    tracing_subscriber::fmt::layer()
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .with_target(false)
        .without_time()
}

/// Initializes stderr logging for a command invocation (`VDP_LOG` overrides the default filter).
pub fn init_logging(global: &GlobalArgs, is_wrapper: bool) {
    init_stderr_logging(global.default_log_filter(is_wrapper), is_wrapper);
}

pub(crate) fn init_stderr_logging(default_filter: &str, is_wrapper: bool) {
    let _ = tracing_subscriber::registry()
        .with(stderr_layer().with_filter(stderr_env_filter(default_filter, is_wrapper)))
        .try_init();
}

/// Initializes stderr logging and, when opted into outside wrapper/dbt mode, opens an NDJSON stream and
/// returns a [`RunLogGuard`] that flushes buffered events on drop.
#[must_use]
pub fn init_logging_with_capture(
    program: &str,
    global: &GlobalArgs,
    is_wrapper: bool,
) -> Option<RunLogGuard> {
    let json_path = (!is_wrapper)
        .then_some(global.log_json.as_deref())
        .flatten()
        .filter(|path| !path.as_os_str().is_empty())
        .map(PathBuf::from);
    let stderr_filter = stderr_env_filter(global.default_log_filter(is_wrapper), is_wrapper);

    let Some(path) = json_path else {
        let _ = tracing_subscriber::registry()
            .with(stderr_layer().with_filter(stderr_filter))
            .try_init();
        return None;
    };

    let writer = match open_ndjson_writer(&path) {
        Ok(writer) => Arc::new(Mutex::new(writer)),
        Err(err) => {
            eprintln!(
                "{program}: warning: could not open JSON run log {}: {err}",
                path.display()
            );
            let _ = tracing_subscriber::registry()
                .with(stderr_layer().with_filter(stderr_filter))
                .try_init();
            return None;
        }
    };

    let capture_layer = JsonCaptureLayer {
        program: program.to_owned(),
        path: path.clone(),
        writer: Arc::clone(&writer),
        warned: AtomicBool::new(false),
    }
    .with_filter(env_filter(global.file_log_filter()));
    let _ = tracing_subscriber::registry()
        .with(stderr_layer().with_filter(stderr_filter))
        .with(capture_layer)
        .try_init();
    Some(RunLogGuard {
        program: program.to_owned(),
        path,
        writer,
    })
}

/// Maps a command result to its numeric exit code (`0` = success, `1` = failures, `2` = error),
/// printing fatal errors to stderr prefixed with `program`.
pub fn exit_code(program: &str, result: Result<Outcome>) -> u8 {
    match result {
        Ok(outcome) => outcome.exit_code(),
        Err(error) => {
            eprintln!("{program}: error: {error:#}");
            2
        }
    }
}

/// Maps a command result to a process [`ExitCode`].
pub fn exit(program: &str, result: Result<Outcome>) -> ExitCode {
    ExitCode::from(exit_code(program, result))
}

/// Launch parameters for [`run`].
#[derive(Debug, Clone, Copy)]
pub struct RunContext<'a> {
    pub program: &'a str,
    pub global: &'a GlobalArgs,
    pub is_wrapper: bool,
}

/// Runs a CLI command after initializing logging (and optional NDJSON log streaming) and maps its outcome to
/// an [`ExitCode`].
pub fn run(ctx: RunContext<'_>, action: impl FnOnce() -> Result<Outcome>) -> ExitCode {
    let guard = init_logging_with_capture(ctx.program, ctx.global, ctx.is_wrapper);
    if !ctx.is_wrapper {
        tracing::debug!(program = ctx.program, "command started");
    }
    let result = action();
    if !ctx.is_wrapper {
        match &result {
            Ok(outcome) => tracing::debug!(
                program = ctx.program,
                exit_code = outcome.exit_code(),
                "command completed"
            ),
            Err(error) => {
                tracing::debug!(
                    program = ctx.program,
                    error = %format!("{error:#}"),
                    "command failed"
                );
            }
        }
    }
    drop(guard);
    exit(ctx.program, result)
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::*;

    #[test]
    fn exit_code_maps_outcomes_and_errors_for_both_binaries() {
        for program in ["vdp", "vdp-vaultspeed"] {
            assert_eq!(exit_code(program, Ok(Outcome::Success)), 0);
            assert_eq!(exit_code(program, Ok(Outcome::Failures)), 1);
            assert_eq!(exit_code(program, Err(anyhow!("boom"))), 2);
        }
    }

    #[test]
    fn utc_rfc3339_formats_epoch_and_leap_day() {
        assert_eq!(
            format_utc_duration(Duration::ZERO),
            "1970-01-01T00:00:00.000Z"
        );
        // 2024-02-29T12:34:56.789Z = 19782 days * 86400 + 12*3600 + 34*60 + 56
        let secs = 19_782 * 86_400 + 12 * 3_600 + 34 * 60 + 56;
        assert_eq!(
            format_utc_duration(Duration::new(secs, 789_000_000)),
            "2024-02-29T12:34:56.789Z"
        );
    }
}
