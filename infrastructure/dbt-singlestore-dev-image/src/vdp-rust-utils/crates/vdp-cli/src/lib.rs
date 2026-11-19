//! The `vdp` command line and the multicall `dbt` wrapper.
//!
//! This crate is the composition root: the only place that names concrete types (Vault, the SingleStore ADBC driver, OpenSSH, the dbt
//! subprocess) and wires them into the [`vdp_core::Context`] that module commands receive.
//!
//! It is also a library for binaries that add a module of their own (the VaultSpeed framework's `vdp-vaultspeed`):
//! - [`args`]: [`args::GlobalArgs`] and [`args::Command`], to flatten into their own command line;
//! - [`bootstrap`]: [`bootstrap::context`] (the services), [`bootstrap::adbc_driver`], and the secret chain
//!   ([`bootstrap::resolve_secrets_from`] with [`bootstrap::SecretSources`], giving [`bootstrap::ResolvedSecrets`]);
//! - [`commands`]: [`commands::run_command`] for every command the binary does not handle itself, and, with the
//!   `release-hub` feature, `commands::hub::serve` to start the release hub with extensions;
//! - [`external`]: the hand-off of `vdp vaultspeed ...` to the framework binary.

pub mod args;
pub mod bootstrap;
pub mod commands;
pub mod external;
