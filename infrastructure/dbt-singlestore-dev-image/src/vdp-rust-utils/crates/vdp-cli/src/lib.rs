//! The `vdp` command line and the multicall `dbt` wrapper.
//!
//! This crate is the composition root: the only place that names concrete types (Vault, the SingleStore ADBC driver, OpenSSH, the dbt
//! subprocess) and wires them into the [`vdp_core::Context`] that module commands receive.

pub mod args;
pub mod bootstrap;
pub mod commands;
