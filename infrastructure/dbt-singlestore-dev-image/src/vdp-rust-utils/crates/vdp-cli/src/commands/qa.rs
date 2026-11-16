//! `vdp qa ...`

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use vdp_core::{Context, Outcome};
use vdp_db::SqlSession;
use vdp_dbt::DbtProject;
use vdp_dbt::artifacts::Manifest;
use vdp_qa::QaSettings;

use super::common::{parse, print_dbt_errors};
use crate::args::QaCommand;

/// The project's manifest, from `dbt parse` when there is none yet.
fn manifest(ctx: &Context) -> Result<Manifest> {
    let project = ctx.service::<Arc<DbtProject>>()?;
    let path = project.manifest_path();
    if !path.is_file() {
        let done = parse(ctx, &project, false)?;
        if !done.success() {
            print_dbt_errors(&done);
            bail!("dbt parse failed");
        }
    }
    Manifest::load(&path).with_context(|| path.display().to_string())
}

pub(super) fn qa(ctx: &Context, command: &QaCommand) -> Result<Outcome> {
    let manifest = manifest(ctx)?;
    match command {
        QaCommand::OrphanTables => {
            let orphans = vdp_qa::orphan_tables(&manifest);
            println!(
                "{} table models with no downstream readers (tests aside):",
                orphans.len()
            );
            for id in orphans {
                println!(" - {id}");
            }
        }
        QaCommand::ViewCandidates { out, no_timings } => {
            let timings = if *no_timings {
                std::collections::HashMap::new()
            } else {
                let settings: QaSettings = ctx.config().section(QaSettings::SECTION)?;
                let table = settings.run_results_table_for(ctx.environment().as_str());
                let session = ctx.service::<Arc<dyn SqlSession>>()?;
                vdp_qa::latest_timings(session.as_ref(), &table).with_context(|| {
                    format!(
                        "reading {table} (set [qa].run_results_table, or use --no-timings to skip)"
                    )
                })?
            };
            let candidates = vdp_qa::view_candidates(&manifest, &timings);
            if candidates.is_empty() {
                println!("no view candidates (or every view already has a t_ table)");
                return Ok(Outcome::Success);
            }
            let path = out
                .clone()
                .unwrap_or_else(|| PathBuf::from("dbt_view_candidates.csv"));
            std::fs::write(&path, vdp_qa::candidates_csv(&candidates))?;
            for c in candidates.iter().take(10) {
                println!(
                    "{:>8.2}s  {:<40} {} downstream",
                    c.total_downstream_time_seconds, c.model_name, c.affected_downstream_count
                );
            }
            println!(
                "{} candidates written to {}",
                candidates.len(),
                path.display()
            );
        }
    }
    Ok(Outcome::Success)
}
