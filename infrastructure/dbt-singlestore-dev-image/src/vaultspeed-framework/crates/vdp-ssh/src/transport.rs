use std::fmt::Debug;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::{SshEndpoint, SshError};

/// One file to upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub content: Vec<u8>,
    pub remote_path: String,
}

/// Copies files to a remote host, in the given order, each atomically.
pub trait RemoteTransport: Send + Sync + Debug {
    /// A short description of the target (`sftp://user@host:port`) for logs and errors.
    fn describe(&self) -> String;
    /// Uploads `files`; an empty slice is a no-op.
    ///
    /// # Errors
    ///
    /// [`SshError::Io`] when the staging files cannot be written; [`SshError::Spawn`] when the `sftp` client cannot be
    /// started; [`SshError::Upload`] when `sftp` exits with a non-zero status (the batch is aborted).
    fn upload(&self, files: &[Upload]) -> Result<(), SshError>;
}

/// Uploads with the system OpenSSH `sftp` client in batch mode.
///
/// Each file is uploaded under a temporary name and renamed into place (an atomic `posix-rename` on OpenSSH
/// servers), and any failure aborts the batch with a non-zero exit status that is reported as an error.
#[derive(Debug, Clone)]
pub struct OpenSshTransport {
    endpoint: SshEndpoint,
    program: PathBuf,
}

impl OpenSshTransport {
    /// Creates a transport that runs the `sftp` client from `PATH` against `endpoint`.
    pub fn new(endpoint: SshEndpoint) -> Self {
        Self {
            endpoint,
            program: PathBuf::from("sftp"),
        }
    }

    /// Batch script for the given local/remote pairs.
    fn batch(files: &[(PathBuf, String)]) -> String {
        let mut script = String::new();
        let mut dirs: Vec<&str> = files
            .iter()
            .filter_map(|(_, r)| r.rsplit_once('/').map(|(d, _)| d))
            .filter(|d| !d.is_empty())
            .collect();
        dirs.dedup();
        for dir in dirs {
            // `-` prefix: an existing directory is not an error.
            let _ = writeln!(script, "-mkdir {}", quote(dir));
        }
        for (local, remote) in files {
            let (dir, name) = remote.rsplit_once('/').unwrap_or(("", remote));
            let temp = if dir.is_empty() {
                format!(".{name}.vdp-upload")
            } else {
                format!("{dir}/.{name}.vdp-upload")
            };
            let _ = writeln!(
                script,
                "put {} {}",
                quote(&local.to_string_lossy()),
                quote(&temp)
            );
            let _ = writeln!(script, "rename {} {}", quote(&temp), quote(remote));
        }
        script
    }
}

fn quote(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

impl RemoteTransport for OpenSshTransport {
    fn describe(&self) -> String {
        format!(
            "sftp://{}:{}",
            self.endpoint.destination(),
            self.endpoint.port
        )
    }

    fn upload(&self, files: &[Upload]) -> Result<(), SshError> {
        if files.is_empty() {
            return Ok(());
        }
        let io = |path: PathBuf| move |source| SshError::Io { path, source };
        let staging = tempfile::tempdir().map_err(io(std::env::temp_dir()))?;
        let mut locals = Vec::with_capacity(files.len());
        for (index, file) in files.iter().enumerate() {
            let local = staging.path().join(index.to_string());
            fs::write(&local, &file.content).map_err(io(local.clone()))?;
            locals.push((local, file.remote_path.clone()));
        }
        let batch_path = staging.path().join("batch");
        fs::write(&batch_path, Self::batch(&locals)).map_err(io(batch_path.clone()))?;

        let output = Command::new(&self.program)
            .arg("-b")
            .arg(&batch_path)
            .args(self.endpoint.common_options())
            .arg("-P")
            .arg(self.endpoint.port.to_string())
            .arg(self.endpoint.destination())
            .output()
            .map_err(|source| SshError::Spawn {
                program: self.program.display().to_string(),
                source,
            })?;
        if output.status.success() {
            tracing::info!(files = files.len(), target = %self.describe(), "uploaded");
            Ok(())
        } else {
            Err(SshError::Upload {
                target: self.describe(),
                detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_script_uploads_atomically() {
        let script = OpenSshTransport::batch(&[(PathBuf::from("/tmp/0"), "/opt/dags/a.py".into())]);
        assert_eq!(
            script,
            "-mkdir \"/opt/dags\"\nput \"/tmp/0\" \"/opt/dags/.a.py.vdp-upload\"\nrename \"/opt/dags/.a.py.vdp-upload\" \"/opt/dags/a.py\"\n"
        );
    }
}
