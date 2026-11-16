//! `vdp seeds ...` and `vdp maintenance ...`

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use vdp_core::{Context, Outcome};
use vdp_dbt::artifacts::Manifest;
use vdp_dbt::tasks::maintenance::{self, MaintenanceSettings};
use vdp_dbt::tasks::seed_metadata::{self, SeedSettings};
use vdp_dbt::{DbtEngine, DbtProject, Invocation};

use super::common::{parse, print_dbt_errors, relative};
use crate::args::{MaintenanceCommand, SeedsCommand};
use crate::bootstrap::DbtSelection;

pub(super) fn seeds(ctx: &Context, command: &SeedsCommand) -> Result<Outcome> {
    let SeedsCommand::RefreshMetadata = command;
    let settings: SeedSettings = ctx.config().section(SeedSettings::SECTION)?;
    let project = ctx.service::<Arc<DbtProject>>()?;
    let write = |files: Vec<(std::path::PathBuf, String)>| -> Result<()> {
        for (path, content) in files {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, content).with_context(|| path.display().to_string())?;
            println!("wrote {}", relative(&project.root, &path));
        }
        Ok(())
    };
    write(seed_metadata::render_properties(&project.root, &settings))?;
    let done = parse(ctx, &project, false)?;
    if !done.success() {
        print_dbt_errors(&done);
        bail!("dbt parse failed");
    }
    let manifest = Manifest::load(&project.manifest_path()).context("reading the manifest")?;
    if manifest
        .nodes_of(&["model", "seed", "snapshot"])
        .next()
        .is_none()
    {
        bail!("no dbt models found in the project");
    }
    write(seed_metadata::render_csv(
        &manifest,
        &project.root,
        &settings,
    ))?;
    Ok(Outcome::Success)
}

pub(super) fn maintenance(ctx: &Context, command: &MaintenanceCommand) -> Result<Outcome> {
    let MaintenanceCommand::Export { out } = command;
    let mut settings: MaintenanceSettings = ctx.config().section(MaintenanceSettings::SECTION)?;
    let project = ctx.service::<Arc<DbtProject>>()?;
    let selection = ctx.service::<DbtSelection>()?;
    if let Some(target) = &selection.target {
        settings.target.clone_from(target);
    }
    let engine = ctx.service::<Arc<dyn DbtEngine>>()?;
    let args: Vec<String> = settings.args();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // The target comes from [maintenance]; only the profiles directory is added.
    let selection = DbtSelection {
        target: None,
        ..selection
    };
    let done = engine.invoke(&Invocation {
        args: selection.args(&project.root, &args),
        cwd: Some(project.root.clone()),
        capture: true,
    })?;
    if !done.success() {
        print_dbt_errors(&done);
        bail!("dbt run-operation {} failed", settings.macro_name);
    }
    let Some(report) = maintenance::report_from_output(&done.stdout_text()) else {
        bail!("{} printed no JSON object", settings.macro_name);
    };
    let path = out
        .clone()
        .unwrap_or_else(|| project.root.join(&settings.out));
    std::fs::write(&path, serde_json::to_string_pretty(&report)? + "\n")
        .with_context(|| path.display().to_string())?;
    println!("wrote {}", path.display());
    Ok(Outcome::Success)
}
