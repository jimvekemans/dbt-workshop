//! Remote access through the system OpenSSH client (`ssh`, `sftp`).
//!
//! A building block. It replaces paramiko in the Python utilities, which accepted any host key (`AutoAddPolicy`), read a
//! hard-coded `/root/.ssh/known_hosts` and ignored remote exit codes. Used by the VaultSpeed deployments (DAG uploads
//! to the Airflow host).
//!
//! # Responsibilities
//!
//! - Check host keys strictly and treat a non-zero remote exit status as an error ([`SshError`]).
//! - Run commands on a remote host ([`RemoteShell`], [`OpenSshShell`]).
//! - Upload files under a temporary name and rename them into place ([`RemoteTransport`], [`OpenSshTransport`]).
//! - Turn the Airflow secrets into an endpoint for both ([`SshAccess`], [`SecretNames`], [`SshEndpoint`]).
//! - Quote words for a POSIX shell ([`shell_quote`]).
//!
//! # Example
//!
//! ```
//! use vdp_ssh::shell_quote;
//!
//! assert_eq!(shell_quote("release/2026_DROP05"), "release/2026_DROP05");
//! assert_eq!(shell_quote("it's"), r"'it'\''s'");
//! ```
//!
//! Running a command (needs a reachable host, so it is only compiled here):
//!
//! ```no_run
//! use vdp_ssh::{OpenSshShell, RemoteShell, SshEndpoint};
//!
//! let shell = OpenSshShell::new(SshEndpoint {
//!     user: "airflow".into(),
//!     host: "airflow.example.com".into(),
//!     port: 22,
//!     identity_file: None,
//!     known_hosts: None,
//! });
//! let output = shell.run("uptime")?;
//! println!("{}", output.stdout);
//! # Ok::<(), vdp_ssh::SshError>(())
//! ```

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
