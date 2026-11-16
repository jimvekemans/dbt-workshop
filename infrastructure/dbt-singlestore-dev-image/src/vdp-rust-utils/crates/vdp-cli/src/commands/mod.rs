//! Command dispatch.

#[cfg(feature = "airflow")]
mod airflow;
mod common;
pub mod dbt;
mod dev;
mod env;
#[cfg(feature = "release-hub")]
mod hub;
mod project;
#[cfg(feature = "qa")]
mod qa;
#[cfg(feature = "release")]
mod release;
#[cfg(feature = "sources")]
mod sources;
#[cfg(feature = "sql")]
mod sql;
#[cfg(feature = "vaultspeed")]
mod vaultspeed;

use anyhow::Result;
use vdp_core::Outcome;

use crate::args::{Cli, Command};
use crate::bootstrap;

/// Configuration sections read by the modules of this toolkit (whether or not they are compiled in).
pub const MODULE_SECTIONS: &[&str] = &[
    "dbt",
    "sources",
    "seeds",
    "maintenance",
    "git",
    "jira",
    "release",
    "release_hub",
    "notify",
    "airflow",
    "sql",
    "qa",
    "vaultspeed",
    "vaultspeed_release",
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
    same::<vdp_dbt::tasks::seed_metadata::SeedSettings>(config, "seeds");
    same::<vdp_dbt::tasks::maintenance::MaintenanceSettings>(config, "maintenance");
    #[cfg(feature = "release")]
    same::<vdp_jira::JiraSettings>(config, "jira");
    #[cfg(feature = "release")]
    same::<vdp_release::ReleaseSettings>(config, "release");
    #[cfg(feature = "release-hub")]
    same::<vdp_release_hub::HubSettings>(config, "release_hub");
    #[cfg(feature = "release-hub")]
    same::<vdp_notify::NotifySettings>(config, "notify");
    #[cfg(feature = "airflow")]
    same::<vdp_airflow::AirflowSettings>(config, "airflow");
    #[cfg(feature = "qa")]
    same::<vdp_qa::QaSettings>(config, "qa");
    #[cfg(feature = "vaultspeed")]
    same::<vdp_vaultspeed::VaultspeedSettings>(config, "vaultspeed");
    #[cfg(feature = "release-hub")]
    same::<vdp_vaultspeed_release::VsReleaseSettings>(config, "vaultspeed_release");
}

pub fn run(cli: Cli) -> Result<Outcome> {
    if let Command::Dbt { args } = cli.command {
        return dbt::wrapper(&cli.global, &args);
    }
    let cwd = std::env::current_dir()?;
    #[cfg(feature = "vaultspeed")]
    let cli = {
        let mut cli = cli;
        if matches!(cli.command, Command::Vaultspeed(_)) {
            cli.global.environment = vaultspeed::environment_override(&cli.global)?;
        }
        cli
    };
    let ctx = bootstrap::context(&cli.global, &cwd)?;
    match cli.command {
        Command::Dbt { .. } => unreachable!("handled above"),
        #[cfg(feature = "sources")]
        Command::Sources(command) => sources::run(&ctx, &command),
        Command::Seeds(command) => project::seeds(&ctx, &command),
        Command::Maintenance(command) => project::maintenance(&ctx, &command),
        #[cfg(feature = "release")]
        Command::Release(command) => release::release(&ctx, &command),
        #[cfg(feature = "release")]
        Command::Jira(command) => release::jira(&ctx, &command),
        #[cfg(feature = "airflow")]
        Command::Airflow(command) => airflow::airflow(&ctx, &command),
        #[cfg(feature = "sql")]
        Command::Sql(command) => sql::sql(&ctx, &command),
        #[cfg(feature = "qa")]
        Command::Qa(command) => qa::qa(&ctx, &command),
        #[cfg(feature = "vaultspeed")]
        Command::Vaultspeed(args) => vaultspeed::run(&ctx, &cli.global, &args),
        Command::Env(command) => env::run(&ctx, &command),
        Command::Dev(command) => dev::run(&ctx, &command),
    }
}
