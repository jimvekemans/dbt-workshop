//! Helpers shared by the module commands.

#[cfg(any(feature = "sources", feature = "release"))]
use std::path::Path;
use std::sync::Arc;

#[cfg(any(feature = "sources", feature = "qa"))]
use anyhow::Result;
use vdp_core::Context;
use vdp_dbt::{DbtEngine, DbtProject, Invocation};

use crate::bootstrap::DbtSelection;

/// `dbt parse` in the project, captured; with `json` the log is in `--log-format json`.
#[cfg(any(feature = "sources", feature = "qa"))]
pub(crate) fn parse(ctx: &Context, project: &DbtProject, json: bool) -> Result<vdp_dbt::Completed> {
    let engine = ctx.service::<Arc<dyn DbtEngine>>()?;
    let selection = ctx.service::<DbtSelection>()?;
    let base: &[&str] = if json {
        &["parse", "--log-format", "json"]
    } else {
        &["parse"]
    };
    let args = selection.args(&project.root, base);
    Ok(engine.invoke(&Invocation {
        args,
        cwd: Some(project.root.clone()),
        capture: true,
    })?)
}

#[cfg(any(feature = "sources", feature = "qa"))]
pub(crate) fn print_dbt_errors(done: &vdp_dbt::Completed) {
    let stdout = done.stdout_text();
    for line in stdout
        .lines()
        .chain(String::from_utf8_lossy(&done.stderr).lines())
    {
        let message = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .filter(|e| e["info"]["level"] == "error")
            .and_then(|e| e["info"]["msg"].as_str().map(str::to_owned));
        match message {
            Some(msg) => tracing::error!("{msg}"),
            None if line.contains("[error]") || line.starts_with("error") => {
                tracing::error!("{line}");
            }
            None => {}
        }
    }
}

/// `file` relative to `root` (as is when it is elsewhere), for messages.
#[cfg(feature = "sources")]
pub(crate) fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
}

/// Runs a dbt validation invocation (`args`) in `dir` with `engine`.
///
/// This is what `vdp release create` validates a release branch with, and what the release UI's hub
/// (`vdp-release-ui release serve --validate`) validates the releases it builds in its clone with.
///
/// # Errors
///
/// The reason as text when dbt cannot be started, or exits with a failure: up to 10 of its `[error]` lines, else
/// its exit code.
#[cfg(feature = "release")]
pub fn run_dbt_validation(
    engine: &dyn DbtEngine,
    dir: &Path,
    args: Vec<std::ffi::OsString>,
) -> std::result::Result<(), String> {
    let done = engine
        .invoke(&Invocation {
            args,
            cwd: Some(dir.to_owned()),
            capture: true,
        })
        .map_err(|e| vdp_core::display_chain(&e))?;
    if done.success() {
        return Ok(());
    }
    let output = done.stdout_text();
    let errors: Vec<&str> = output
        .lines()
        .filter(|l| l.contains("[error]"))
        .take(10)
        .collect();
    Err(if errors.is_empty() {
        format!("dbt exited with {}", done.exit_code())
    } else {
        errors.join("\n")
    })
}

/// Runs the configured dbt validation in the current project.
#[cfg(feature = "release")]
pub(crate) fn validate_project(ctx: &Context, args: &[String]) -> std::result::Result<(), String> {
    let project = ctx
        .service::<Arc<DbtProject>>()
        .map_err(|e| vdp_core::display_chain(&e))?;
    let engine = ctx
        .service::<Arc<dyn DbtEngine>>()
        .map_err(|e| vdp_core::display_chain(&e))?;
    let selection = ctx.service::<DbtSelection>().map_err(|e| e.to_string())?;
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    tracing::info!("validating: dbt {}", borrowed.join(" "));
    run_dbt_validation(
        engine.as_ref(),
        &project.root,
        selection.args(&project.root, &borrowed),
    )
}
