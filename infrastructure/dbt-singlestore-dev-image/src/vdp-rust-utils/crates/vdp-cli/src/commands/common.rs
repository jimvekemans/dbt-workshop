//! Helpers shared by the module commands.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use vdp_core::Context;
use vdp_dbt::{DbtEngine, DbtProject, Invocation};

use crate::bootstrap::DbtSelection;

/// `dbt parse` in the project, captured; with `json` the log is in `--log-format json`.
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
            Some(msg) => eprintln!("{msg}"),
            None if line.contains("[error]") || line.starts_with("error") => eprintln!("{line}"),
            None => {}
        }
    }
}

pub(crate) fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .display()
        .to_string()
}
