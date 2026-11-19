//! The dbt wrapper entry point.

use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context as _, Result};
use vdp_core::{Config, Outcome};
use vdp_dbt::{DbtSettings, wrapper};

use crate::args::GlobalArgs;
use crate::bootstrap;

/// Global options for the `dbt` link, which has no options of its own: `$DBT_DEPLOY_TARGET`, `$VDP_CONFIG`,
/// `$VDP_PROJECT_DIR` and `$VDP_NO_VAULT`.
pub fn multicall_globals() -> GlobalArgs {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    GlobalArgs {
        environment: None,
        project_dir: var("VDP_PROJECT_DIR").map(Into::into),
        config: var("VDP_CONFIG").map(Into::into),
        target: None,
        profiles_dir: None,
        no_vault: var("VDP_NO_VAULT").is_some_and(|v| v != "0" && v != "false"),
        verbose: false,
        quiet: false,
        log_json: None,
    }
}

/// Runs dbt with `args`.
///
/// The project is the one dbt will use: `--project-dir` from the dbt arguments when given, so the VDP
/// configuration, dotenv file and secrets match it.
///
/// # Errors
///
/// An error when the working directory or the configuration cannot be read, no dbt engine is found, the context cannot
/// be built, or dbt cannot be started. dbt's own failure is not an error: it is the exit code of the returned
/// [`Outcome`].
pub fn wrapper(global: &GlobalArgs, args: &[OsString]) -> Result<Outcome> {
    let cwd = std::env::current_dir()?;
    let project = bootstrap::locate_project(global.project_dir.as_deref(), &cwd);
    let config = Config::load(&bootstrap::config_files(
        project.as_ref(),
        global.config.as_deref(),
    ))?;
    let settings: DbtSettings = config.section(DbtSettings::SECTION)?;
    let binary = vdp_dbt::locate_engine(Some(&settings.binary))?;
    let grammar = wrapper::grammar_for(&binary, &settings);
    let scan = vdp_dbt::argv::scan(args, &grammar);

    let mut global = global.clone();
    if let Some(dir) = scan.value(args, "project-dir") {
        global.project_dir = Some(cwd.join(dir));
    }
    let ctx = bootstrap::context(&global, &cwd)?;
    wrapper::run(
        &ctx,
        wrapper::RunRequest {
            binary: &binary,
            args,
            scan: &scan,
        },
    )
    .with_context(|| format!("running {}", binary.display()))
}

/// The engine binary for module commands.
///
/// # Errors
///
/// An error when the `[dbt]` section is invalid or no dbt engine is found (see [`vdp_dbt::locate_engine`]).
pub fn engine_binary(config: &Config) -> Result<std::path::PathBuf> {
    let settings: DbtSettings = config.section(DbtSettings::SECTION)?;
    Ok(vdp_dbt::locate_engine(Some(Path::new(&settings.binary)))?)
}
