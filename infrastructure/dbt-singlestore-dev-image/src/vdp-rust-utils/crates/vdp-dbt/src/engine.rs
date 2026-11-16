//! Running dbt.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;
use vdp_secrets::{ExposeSecret, SecretString};

use crate::DbtError;

/// One dbt run.
#[derive(Debug, Clone, Default)]
pub struct Invocation {
    pub args: Vec<OsString>,
    /// Working directory (default: inherited).
    pub cwd: Option<PathBuf>,
    /// Capture stdout/stderr instead of sharing the terminal.
    pub capture: bool,
}

impl Invocation {
    /// A captured run of `args` in `cwd`, as modules use it (`dbt parse --log-format json`).
    pub fn captured<I, S>(args: I, cwd: &Path) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            cwd: Some(cwd.to_owned()),
            capture: true,
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completed {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Completed {
    pub const fn success(&self) -> bool {
        matches!(self.code, Some(0))
    }

    /// The exit code a shell would report: the code, or 128 + the signal number.
    pub fn exit_code(&self) -> u8 {
        match (self.code, self.signal) {
            (Some(code), _) => u8::try_from(code & 0xff).unwrap_or(1),
            (None, Some(signal)) => u8::try_from((128 + signal) & 0xff).unwrap_or(1),
            (None, None) => 1,
        }
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    fn from_status(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
        use std::os::unix::process::ExitStatusExt;
        Self {
            code: status.code(),
            signal: status.signal(),
            stdout,
            stderr,
        }
    }
}

/// Runs dbt. Modules depend on this trait, never on the subprocess.
pub trait DbtEngine: Send + Sync + fmt::Debug {
    fn binary(&self) -> &Path;
    fn invoke(&self, invocation: &Invocation) -> Result<Completed, DbtError>;
}

/// dbt as a child process, with the secrets in its environment only.
pub struct SubprocessDbt {
    binary: PathBuf,
    env: Vec<(String, SecretString)>,
}

impl fmt::Debug for SubprocessDbt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubprocessDbt")
            .field("binary", &self.binary)
            .field("env", &self.env.iter().map(|(k, _)| k).collect::<Vec<_>>())
            .finish()
    }
}

impl SubprocessDbt {
    pub fn new(binary: PathBuf, env: Vec<(String, SecretString)>) -> Self {
        Self { binary, env }
    }

    fn command(&self, invocation: &Invocation) -> Command {
        let mut command = Command::new(&self.binary);
        command.args(&invocation.args);
        if let Some(cwd) = &invocation.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &self.env {
            command.env(key, value.expose_secret());
        }
        command
    }

    /// Shares the terminal with dbt. The wrapper survives Ctrl-C (the terminal delivers SIGINT to dbt as well)
    /// and forwards SIGTERM, SIGHUP and SIGQUIT, which a scheduler sends to the wrapper's pid only.
    fn run_interactive(&self, invocation: &Invocation) -> Result<Completed, DbtError> {
        let interrupted = Arc::new(AtomicBool::new(false));
        let sigint = signal_hook::flag::register(SIGINT, interrupted)
            .map_err(|e| DbtError::Signals(e.to_string()))?;
        let mut signals = Signals::new([SIGTERM, SIGHUP, SIGQUIT])
            .map_err(|e| DbtError::Signals(e.to_string()))?;
        let handle = signals.handle();

        let spawned = self
            .command(invocation)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(source) => {
                handle.close();
                signal_hook::low_level::unregister(sigint);
                return Err(DbtError::Spawn {
                    binary: self.binary.clone(),
                    source,
                });
            }
        };
        let pid = i32::try_from(child.id()).ok().map(Pid::from_raw);
        let forwarder = std::thread::spawn(move || {
            for number in signals.forever() {
                if let (Some(pid), Ok(signal)) = (pid, Signal::try_from(number)) {
                    let _ = kill(pid, signal);
                }
            }
        });
        let status = child.wait();
        handle.close();
        let _ = forwarder.join();
        signal_hook::low_level::unregister(sigint);
        let status = status.map_err(|source| DbtError::Spawn {
            binary: self.binary.clone(),
            source,
        })?;
        Ok(Completed::from_status(status, Vec::new(), Vec::new()))
    }
}

impl DbtEngine for SubprocessDbt {
    fn binary(&self) -> &Path {
        &self.binary
    }

    fn invoke(&self, invocation: &Invocation) -> Result<Completed, DbtError> {
        if !invocation.capture {
            return self.run_interactive(invocation);
        }
        let output = self
            .command(invocation)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| DbtError::Spawn {
                binary: self.binary.clone(),
                source,
            })?;
        Ok(Completed::from_status(
            output.status,
            output.stdout,
            output.stderr,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_shell_conventions() {
        let done = |code, signal| Completed {
            code,
            signal,
            ..Completed::default()
        };
        assert_eq!(done(Some(0), None).exit_code(), 0);
        assert_eq!(done(Some(2), None).exit_code(), 2);
        assert_eq!(done(None, Some(15)).exit_code(), 143);
    }

    #[test]
    fn captured_runs_see_only_the_given_environment_additions() {
        let engine = SubprocessDbt::new(
            PathBuf::from("sh"),
            vec![("VDP_TEST_SECRET".into(), SecretString::from("s3cret"))],
        );
        let done = engine
            .invoke(&Invocation::captured(
                ["-c", "printf %s \"$VDP_TEST_SECRET\"; exit 3"],
                Path::new("/"),
            ))
            .unwrap();
        assert_eq!(done.stdout_text(), "s3cret");
        assert_eq!(done.exit_code(), 3);
        assert!(std::env::var_os("VDP_TEST_SECRET").is_none());
        assert!(!format!("{engine:?}").contains("s3cret"));
    }
}
