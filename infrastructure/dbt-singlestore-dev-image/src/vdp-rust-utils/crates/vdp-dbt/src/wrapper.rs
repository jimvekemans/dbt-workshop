//! The `dbt` command: dbt v2 plus the organisation's environment, the selection guard and helpful hints.
//!
//! Port of `dbt_wrapper.py` / `dbt_manager.wrapper` / `ProjectManager.setup_env`, with one overriding rule:
//! every argv dbt accepts reaches dbt with the same meaning. Tokens are forwarded byte for byte; the only values
//! that ever change are `--args`/`--vars` values dbt would misread or reject (see [`crate::argv::yaml_arg`]).

use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use vdp_core::{Context, Outcome};
use vdp_secrets::{ExposeSecret, SecretProvider, SecretString};

use crate::argv::{self, Scan};
use crate::flags::{Grammar, cache};
use crate::guard::{self, Verdict};
use crate::{DbtEngine, DbtError, DbtSettings, Invocation, SubprocessDbt, missing_sources};

/// The grammar to read argv with, per `[dbt].grammar`.
pub fn grammar_for(binary: &Path, settings: &DbtSettings) -> Grammar {
    if settings.grammar == "embedded" {
        Grammar::embedded()
    } else {
        cache::for_binary(binary)
    }
}

/// Whether the invocation needs no secrets (help, version, completions, ...).
pub fn is_offline(scan: &Scan, settings: &DbtSettings) -> bool {
    scan.uses("help")
        || scan.uses("version")
        || scan
            .command()
            .is_none_or(|command| settings.offline_commands.iter().any(|c| c == command))
}

/// The variables dbt gets on top of the inherited environment: `[dbt].env` defaults (unless already set),
/// `VDP_ENV`, `VDP_SSL_CA` (the environment's CA, for `ssl_ca:` in profiles.yml) and every secret the dotenv
/// file and Vault provide.
pub fn child_env(
    ctx: &Context,
    settings: &DbtSettings,
    secrets: Option<&dyn SecretProvider>,
) -> Vec<(String, SecretString)> {
    let mut env: Vec<(String, SecretString)> = settings
        .env
        .iter()
        .filter(|(key, _)| std::env::var_os(key).is_none())
        .map(|(k, v)| (k.clone(), SecretString::from(v.clone())))
        .collect();
    env.push((
        "VDP_ENV".into(),
        SecretString::from(ctx.environment().to_string()),
    ));
    if let Some(ca) = ctx.ca_certificate().filter(|ca| ca.is_file()) {
        env.push((
            "VDP_SSL_CA".into(),
            SecretString::from(ca.display().to_string()),
        ));
    }
    if let Some(secrets) = secrets {
        env.extend(vdp_secrets::child_env(secrets));
    }
    env
}

/// Everything the wrapper decided before running dbt.
#[derive(Debug)]
pub struct Prepared {
    pub args: Vec<OsString>,
    pub verdict: Verdict,
    pub notices: Vec<argv::Notice>,
}

/// Applies the guard and the value fixes.
pub fn prepare(
    args: &[OsString],
    scan: &Scan,
    settings: &DbtSettings,
    secrets: Option<&dyn SecretProvider>,
) -> Prepared {
    let user = secrets
        .and_then(|s| s.get(&settings.user_secret))
        .map(|u| u.expose_secret().to_owned());
    let verdict = guard::decide(scan, settings, user.as_deref());
    let (args, notices) = argv::prepare(args, scan);
    Prepared {
        args,
        verdict,
        notices,
    }
}

/// Runs the wrapper for `args` (without the program name), already scanned with the engine's grammar.
pub fn run(
    ctx: &Context,
    binary: &Path,
    args: &[OsString],
    scan: &Scan,
) -> Result<Outcome, DbtError> {
    let settings: DbtSettings = ctx
        .config()
        .section(DbtSettings::SECTION)
        .map_err(|e| DbtError::Grammar(e.to_string()))?;
    let offline = is_offline(scan, &settings);
    let secrets = if offline {
        None
    } else {
        match ctx.service::<std::sync::Arc<dyn SecretProvider>>() {
            Ok(secrets) => Some(secrets),
            Err(error) => {
                tracing::warn!(%error, "running dbt without the VDP secrets");
                None
            }
        }
    };
    let secrets_ref = secrets.as_deref();

    let prepared = prepare(args, scan, &settings, secrets_ref);
    if let Verdict::Refused { command } = &prepared.verdict {
        eprintln!(
            "vdp: 'dbt {command}' without a selector would build the whole project. Select nodes with \
             --select/-s or --selector, or run it as a service user (…{}, not {}…).",
            settings.service_user_suffix, settings.personal_user_prefix
        );
        return Ok(Outcome::Exit(1));
    }
    for notice in &prepared.notices {
        tracing::warn!(
            "{} {:?} → {:?}: {}",
            notice.flag,
            notice.before,
            notice.after,
            notice.reason
        );
    }

    let log = log_file(ctx, args, scan);
    let log_start = log
        .as_deref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map_or(0, |m| m.len());

    let engine = SubprocessDbt::new(binary.to_owned(), child_env(ctx, &settings, secrets_ref));
    let done = engine.invoke(&Invocation {
        args: prepared.args,
        cwd: None,
        capture: false,
    })?;

    if !done.success()
        && settings.missing_source_hint
        && let Some(log) = &log
    {
        hint_missing_sources(log, log_start);
    }
    Ok(Outcome::Exit(done.exit_code()))
}

/// `dbt.log` of this invocation: `--log-path`, else `$DBT_LOG_PATH`, else `<project>/logs`.
fn log_file(ctx: &Context, args: &[OsString], scan: &Scan) -> Option<PathBuf> {
    let dir = scan
        .value(args, "log-path")
        .map(|p| ctx.cwd().join(p))
        .or_else(|| std::env::var_os("DBT_LOG_PATH").map(|p| ctx.cwd().join(p)))
        .or_else(|| ctx.project_opt().map(|p| p.root.join("logs")))?;
    Some(dir.join("dbt.log"))
}

fn hint_missing_sources(log: &Path, start: u64) {
    let Ok(mut file) = std::fs::File::open(log) else {
        return;
    };
    let mut text = String::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_string(&mut text).is_err() {
        return;
    }
    let missing = missing_sources(&text);
    if missing.is_empty() {
        return;
    }
    let names: Vec<String> = missing
        .iter()
        .map(super::MissingSource::qualified)
        .collect();
    eprintln!(
        "\nhint: dbt reports {} undefined source table(s). Add them from the database with:\n  vdp sources add {}\n  (or: vdp sources add --missing)",
        names.len(),
        names.join(" ")
    );
}

/// An engine for modules: the subprocess with the same environment the wrapper gives dbt.
pub fn module_engine(
    ctx: &Context,
    binary: PathBuf,
    secrets: Option<&dyn SecretProvider>,
) -> Result<SubprocessDbt, DbtError> {
    let settings: DbtSettings = ctx
        .config()
        .section(DbtSettings::SECTION)
        .map_err(|e| DbtError::Grammar(e.to_string()))?;
    Ok(SubprocessDbt::new(
        binary,
        child_env(ctx, &settings, secrets),
    ))
}
