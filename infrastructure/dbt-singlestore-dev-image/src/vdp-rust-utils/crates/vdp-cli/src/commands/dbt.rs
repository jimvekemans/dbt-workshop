//! The dbt wrapper entry point.

use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context as _, Result, anyhow};
use vdp_core::{Config, Outcome};
use vdp_dbt::{DbtSettings, wrapper};

use crate::args::{GlobalArgs, parse_no_vault};
use crate::bootstrap;

/// Global options for the `dbt` link, which has no options of its own: `$DBT_DEPLOY_TARGET`, `$VDP_CONFIG`,
/// `$VDP_PROJECT_DIR` and `$VDP_NO_VAULT` (a boolean, read exactly as `vdp --no-vault` reads it: [`parse_no_vault`]).
///
/// # Errors
///
/// An error when `$VDP_NO_VAULT` is not a boolean.
pub fn multicall_globals() -> Result<GlobalArgs> {
    multicall_globals_from(|name| std::env::var_os(name))
}

fn multicall_globals_from(get: impl Fn(&str) -> Option<OsString>) -> Result<GlobalArgs> {
    let var = |name: &str| get(name).filter(|v| !v.is_empty());
    let no_vault = match var("VDP_NO_VAULT") {
        None => false,
        Some(raw) => raw
            .to_str()
            .ok_or_else(|| anyhow!("$VDP_NO_VAULT is not valid UTF-8"))
            .and_then(|raw| parse_no_vault(raw).map_err(|e| anyhow!("$VDP_NO_VAULT: {e}")))?,
    };
    Ok(GlobalArgs {
        environment: None,
        project_dir: var("VDP_PROJECT_DIR").map(Into::into),
        config: var("VDP_CONFIG").map(Into::into),
        target: None,
        profiles_dir: None,
        no_vault,
        verbose: false,
        quiet: false,
        log_json: None,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn link_reads(no_vault: Option<&str>) -> Result<bool> {
        multicall_globals_from(|name| {
            (name == "VDP_NO_VAULT")
                .then(|| no_vault.map(OsString::from))
                .flatten()
        })
        .map(|global| global.no_vault)
    }

    #[test]
    fn the_link_reads_vdp_no_vault_exactly_as_vdp_does() {
        assert!(!link_reads(None).unwrap());
        for value in [
            "", "1", "true", "yes", "on", "y", "t", "TRUE", "0", "false", "no", "off", "n", "f",
            "Off",
        ] {
            assert_eq!(
                link_reads(Some(value)).unwrap(),
                parse_no_vault(value).unwrap(),
                "{value:?}"
            );
        }
        // Before this rule the link turned Vault off for any non-empty value but `0` and `false`.
        assert!(link_reads(Some("on")).unwrap() && !link_reads(Some("off")).unwrap());
        assert!(!link_reads(Some("no")).unwrap() && !link_reads(Some("")).unwrap());
    }

    #[test]
    fn the_link_refuses_a_vdp_no_vault_that_is_not_a_boolean() {
        let error = link_reads(Some("maybe")).unwrap_err().to_string();
        assert!(
            error.starts_with("$VDP_NO_VAULT: `maybe` is not a boolean"),
            "{error}"
        );
    }
}
