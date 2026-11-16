//! Remote access through the system OpenSSH client (`ssh`, `sftp`).
//!
//! Replaces paramiko in the Python utilities, which accepted any host key (`AutoAddPolicy`), read a hard-coded
//! `/root/.ssh/known_hosts` and ignored remote exit codes. Here host keys are checked strictly, uploads land under
//! a temporary name and are renamed into place, and a non-zero remote exit status is an error.
//!
//! Used by `vdp airflow` (remote shell) and by VaultSpeed deployments (DAG uploads); [`SshAccess`] turns the
//! Airflow secrets into an endpoint for both.

mod access;
mod endpoint;
mod error;
mod shell;
mod transport;

pub use access::{SecretNames, SshAccess};
pub use endpoint::SshEndpoint;
pub use error::SshError;
pub use shell::{CommandOutput, OpenSshShell, RemoteShell, shell_quote};
pub use transport::{OpenSshTransport, RemoteTransport, Upload};
