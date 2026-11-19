//! `vdp-vaultspeed`: the `vdp` command line (as a library, `vdp-cli`) plus the VaultSpeed module.
//!
//! The framework's composition root and CLI entry point: the only crate that names concrete implementations of the
//! framework's ports. The library holds what the `vdp-vaultspeed` binary runs.
//!
//! # Responsibilities
//!
//! It adds to `vdp`:
//! - `vaultspeed deploy|plan|ledger|rules|dags|history` ([`commands::vaultspeed`], [`commands::history`]);
//! - `release serve` with the VaultSpeed extension of the release hub registered ([`hub`]);
//! - the DuckDB object history of applying runs ([`commands::vaultspeed`] opens it, [`commands::history`] reads it).
//!
//! Every other command is `vdp`'s own and runs through `vdp_release_ui_cli::run_command` (the release UI's
//! command line, which adds `release serve` to `vdp`'s) and `vdp_cli::commands`. [`args`] holds the command line and
//! [`bootstrap`] the infrastructure a run needs (secrets, the database session, the ledger, Airflow access).
//!
//! # Example
//!
//! ```
//! use clap::Parser;
//! use vdp_vaultspeed_cli::args::{Cli, Command};
//!
//! let cli = Cli::try_parse_from(["vdp-vaultspeed", "vaultspeed", "plan", "--offline"])?;
//! assert!(matches!(cli.command, Command::Vaultspeed(_)));
//! // Every command of `vdp` is accepted too.
//! let cli = Cli::try_parse_from(["vdp-vaultspeed", "env", "show"])?;
//! assert!(matches!(cli.command, Command::Core(_)));
//! # Ok::<(), clap::Error>(())
//! ```

pub mod args;
pub mod bootstrap;
pub mod commands;
mod help;
pub mod hub;
