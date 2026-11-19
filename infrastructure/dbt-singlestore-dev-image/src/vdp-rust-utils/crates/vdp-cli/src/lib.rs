//! The `vdp` command line and the multicall `dbt` wrapper: the entry point of the toolkit.
//!
//! This is the composition root: the only crate that names concrete types (Vault, the SingleStore ADBC driver,
//! the dbt subprocess) and wires them into the [`vdp_core::Context`] that module commands receive. It is also a library,
//! for binaries that add commands of their own: the VaultSpeed framework's `vdp-vaultspeed` and the release UI's
//! `vdp-release-ui`, each in its own workspace next to this one.
//!
//! # Responsibilities
//!
//! - [`args`]: [`args::GlobalArgs`] and [`args::Command`], to build another command line on (see the note on `Command::External`).
//! - [`bootstrap`]: [`bootstrap::context`] (the services), [`bootstrap::adbc_driver`], and the secret chain
//!   ([`bootstrap::resolve_secrets_from`] with [`bootstrap::SecretSources`], giving [`bootstrap::ResolvedSecrets`]).
//! - [`commands`]: [`commands::run_command`] for every command the binary does not handle itself, and
//!   [`commands::run_dbt_validation`] (the dbt validation of releases, which the release UI's hub reuses).
//! - [`external`]: the hand-off of `vdp vaultspeed ...` and `vdp release-ui ...` to the binaries of the other
//!   workspaces (`vdp-vaultspeed`, `vdp-release-ui`).
//! - [`runtime`]: shared logging initialization and exit-code mapping for both CLI binaries.
//!
//! # Example
//!
//! ```
//! use clap::Parser;
//! use vdp_cli::args::{Cli, Command};
//!
//! let cli = Cli::try_parse_from(["vdp", "--env", "ldv", "env", "show"])?;
//! assert!(matches!(cli.command, Command::Env(_)));
//! assert!(!cli.is_wrapper());
//! # Ok::<(), clap::Error>(())
//! ```

pub mod args;
pub mod bootstrap;
pub mod commands;
pub mod external;
mod help;
pub mod runtime;
