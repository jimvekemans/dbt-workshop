//! Command dispatch.
//!
//! [`run`] is what the `vdp` binary calls. A binary that embeds the toolkit (the VaultSpeed framework's
//! `vdp-vaultspeed`, `vdp-release-ui`) calls [`run_command`] for every command it does not handle itself.

#[cfg(any(feature = "sources", feature = "qa", feature = "release"))]
mod common;
#[cfg(feature = "release")]
pub use common::run_dbt_validation;
pub mod dbt;
mod dev;
mod env;
#[cfg(feature = "qa")]
mod qa;
#[cfg(feature = "release")]
mod release;
#[cfg(feature = "release")]
mod release_present;
#[cfg(feature = "sources")]
mod sources;
#[cfg(feature = "sql")]
mod sql;

use anyhow::Result;
use vdp_core::Outcome;

use vdp_core::Context;

use crate::args::{Cli, Command, GlobalArgs};
use crate::bootstrap;

/// Configuration sections read by the modules of this toolkit (whether or not they are compiled in).
pub const MODULE_SECTIONS: &[&str] = &[
    "dbt",
    "sources",
    "git",
    "jira",
    "release",
    "sql",
    "qa",
    // Read by the VaultSpeed framework binary (`vdp-vaultspeed`) and the release UI's binary (`vdp-release-ui`),
    // not by `vdp`. The same `vdp.toml` serves all of them, so `vdp` must not warn about these sections.
    "vaultspeed",
    "vaultspeed_release",
    "release_hub",
    "notify",
];

/// Asserts that every module section in `config` equals the module's defaults (for the example-config test).
pub fn assert_module_defaults(config: &vdp_core::Config) {
    fn same<T: serde::de::DeserializeOwned + Default + PartialEq + std::fmt::Debug>(
        config: &vdp_core::Config,
        section: &str,
    ) {
        let parsed: T = config.section(section).unwrap();
        assert_eq!(
            parsed,
            T::default(),
            "[{section}] in config/vdp.toml differs from the defaults"
        );
    }
    same::<vdp_dbt::DbtSettings>(config, "dbt");
    #[cfg(feature = "sources")]
    same::<vdp_sources::SourcesSettings>(config, "sources");
    #[cfg(feature = "release")]
    same::<vdp_jira::JiraSettings>(config, "jira");
    #[cfg(feature = "release")]
    same::<vdp_release::ReleaseSettings>(config, "release");
    #[cfg(feature = "qa")]
    same::<vdp_qa::QaSettings>(config, "qa");
}

/// The `vdp` binary's entry point: `command` with the global options.
///
/// # Errors
///
/// As [`run_command`].
pub fn run(cli: Cli) -> Result<Outcome> {
    run_command(&cli.global, cli.command)
}

/// Runs one command with the toolkit's own services (see [`bootstrap::context`]).
///
/// `dbt` does not build a context; every other command gets one for the selected environment. An
/// [`Command::External`] is not a command of this binary and is an error here: the caller hands it over first
/// (see [`crate::external`]).
///
/// # Errors
///
/// An error when the working directory or the context cannot be set up, or the command fails. [`Command::External`] is
/// an error.
pub fn run_command(global: &GlobalArgs, command: Command) -> Result<Outcome> {
    if let Command::Dbt { args } = command {
        return dbt::wrapper(global, &args);
    }
    let cwd = std::env::current_dir()?;
    let ctx = bootstrap::context(global, &cwd)?;
    dispatch(&ctx, command)
}

/// Runs a command (other than `dbt`) in a context made by [`bootstrap::context`].
///
/// # Errors
///
/// An error when the command fails; [`Command::External`] is an error because it is not a command of this binary.
pub fn dispatch(ctx: &Context, command: Command) -> Result<Outcome> {
    match command {
        Command::Dbt { .. } => unreachable!("`dbt` runs without a context (see `run_command`)"),
        #[cfg(feature = "sources")]
        Command::Sources(command) => sources::run(ctx, &command),
        #[cfg(feature = "release")]
        Command::Release(command) => release::release(ctx, &command),
        #[cfg(feature = "release")]
        Command::Jira(command) => release::jira(ctx, &command),
        #[cfg(feature = "sql")]
        Command::Sql(command) => sql::sql(ctx, &command),
        #[cfg(feature = "qa")]
        Command::Qa(command) => qa::qa(ctx, &command),
        Command::Env(command) => env::run(ctx, &command),
        Command::Dev(command) => dev::run(ctx, &command),
        Command::External(args) => anyhow::bail!(
            "{} is not a command of this binary",
            args.first()
                .map_or_else(String::new, |a| a.to_string_lossy().into_owned())
        ),
    }
}
