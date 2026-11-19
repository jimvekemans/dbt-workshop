use std::fmt::Debug;
use std::path::PathBuf;
use std::process::Command;

use crate::{SshEndpoint, SshError};

/// What a remote command printed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Runs shell commands on a remote host. A non-zero exit status is an error.
pub trait RemoteShell: Send + Sync + Debug {
    /// A short description of the target (`ssh://user@host:port`) for logs and errors.
    fn describe(&self) -> String;
    /// Runs `command` on the remote host and returns what it printed.
    ///
    /// # Errors
    ///
    /// [`SshError::Spawn`] when the `ssh` client cannot be started; [`SshError::Remote`] when the remote command exits with
    /// a non-zero status.
    fn run(&self, command: &str) -> Result<CommandOutput, SshError>;
}

/// [`RemoteShell`] over the system `ssh` client.
#[derive(Debug, Clone)]
pub struct OpenSshShell {
    endpoint: SshEndpoint,
    program: PathBuf,
}

impl OpenSshShell {
    /// Creates a shell that runs the `ssh` client from `PATH` against `endpoint`.
    pub fn new(endpoint: SshEndpoint) -> Self {
        Self {
            endpoint,
            program: PathBuf::from("ssh"),
        }
    }

    /// Uses another client binary (tests).
    #[must_use]
    pub fn with_program(mut self, program: PathBuf) -> Self {
        self.program = program;
        self
    }

    fn arguments(&self, command: &str) -> Vec<String> {
        let mut args = self.endpoint.common_options();
        args.extend(["-p".into(), self.endpoint.port.to_string()]);
        args.push(self.endpoint.destination());
        args.push("--".into());
        args.push(command.into());
        args
    }
}

impl RemoteShell for OpenSshShell {
    fn describe(&self) -> String {
        format!(
            "ssh://{}:{}",
            self.endpoint.destination(),
            self.endpoint.port
        )
    }

    fn run(&self, command: &str) -> Result<CommandOutput, SshError> {
        tracing::debug!(target = %self.describe(), %command, "running remote command");
        let output = Command::new(&self.program)
            .args(self.arguments(command))
            .output()
            .map_err(|source| SshError::Spawn {
                program: self.program.display().to_string(),
                source,
            })?;
        let result = CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
        if output.status.success() {
            Ok(result)
        } else {
            Err(SshError::Remote {
                target: self.describe(),
                status: output.status.to_string(),
                detail: result.stderr.trim().to_owned(),
            })
        }
    }
}

/// Quotes a word for a POSIX shell: `it's` becomes `'it'\''s'`.
pub fn shell_quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@+,".contains(c))
    {
        return word.to_owned();
    }
    format!("'{}'", word.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint() -> SshEndpoint {
        SshEndpoint {
            user: "airflow".into(),
            host: "af.example".into(),
            port: 2222,
            identity_file: Some("/tmp/key".into()),
            known_hosts: Some("/etc/ssh/known".into()),
        }
    }

    #[test]
    fn host_keys_are_checked_strictly() {
        let args = OpenSshShell::new(endpoint()).arguments("true");
        assert_eq!(
            args.join(" "),
            "-o BatchMode=yes -o StrictHostKeyChecking=yes -i /tmp/key -o IdentitiesOnly=yes \
             -o UserKnownHostsFile=/etc/ssh/known -p 2222 airflow@af.example -- true"
        );
    }

    #[test]
    fn quoting_survives_a_posix_shell() {
        assert_eq!(shell_quote("v1.2.3"), "v1.2.3");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b;rm -rf /"), "'a b;rm -rf /'");
    }

    #[test]
    fn a_failing_command_is_an_error() {
        // `false` ignores its arguments and exits 1, standing in for the remote side.
        let shell = OpenSshShell::new(endpoint()).with_program("false".into());
        let error = shell.run("anything").unwrap_err();
        assert!(matches!(error, SshError::Remote { .. }), "{error}");
    }
}
