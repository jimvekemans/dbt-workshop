//! `vdp-vaultspeed`: the `vdp` command line (as a library, `vdp-cli`) plus the VaultSpeed module.
//!
//! This crate is the framework's composition root. It adds to `vdp`:
//! - `vaultspeed deploy|plan|ledger|rules|dags|history` ([`commands::vaultspeed`], [`commands::history`]);
//! - `release serve` with the VaultSpeed extension of the release hub registered ([`hub`]);
//! - the DuckDB object history of applying runs ([`commands::vaultspeed`] opens it, [`commands::history`] reads it).
//!
//! Every other command is `vdp`'s own and runs through `vdp_cli::commands`.

pub mod args;
pub mod bootstrap;
pub mod commands;
pub mod hub;
