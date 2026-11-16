//! `vdp sources ...`

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use vdp_core::{Context, Outcome};
use vdp_db::SqlSession;
use vdp_dbt::artifacts::Manifest;
use vdp_dbt::{DbtProject, missing_sources};
use vdp_sources::rewrite::{Refreshed, Trimmed};
use vdp_sources::{AddOutcome, AddRequest, SourcesSettings, SqlCatalog};

use super::common::{parse, print_dbt_errors, relative};
use crate::args::SourcesCommand;

pub(super) fn run(ctx: &Context, command: &SourcesCommand) -> Result<Outcome> {
    let settings: SourcesSettings = ctx.config().section(SourcesSettings::SECTION)?;
    match command {
        SourcesCommand::Add {
            tables,
            missing,
            dry_run,
        } => add(ctx, &settings, tables, *missing, *dry_run),
        SourcesCommand::Rewrite { databases, no_trim } => rewrite(
            ctx,
            &settings,
            databases.as_deref().unwrap_or(&settings.databases),
            *no_trim,
        ),
    }
}

fn catalog(ctx: &Context, settings: &SourcesSettings) -> Result<SqlCatalog> {
    let session = ctx.service::<Arc<dyn SqlSession>>()?;
    Ok(SqlCatalog::new(session, &settings.column_order))
}

fn undefined(done: &vdp_dbt::Completed) -> Vec<AddRequest> {
    let text = format!(
        "{}\n{}",
        done.stdout_text(),
        String::from_utf8_lossy(&done.stderr)
    );
    missing_sources(&text)
        .into_iter()
        .map(|m| AddRequest {
            source: m.source,
            table: m.table,
        })
        .collect()
}

fn add(
    ctx: &Context,
    settings: &SourcesSettings,
    tables: &[String],
    missing: bool,
    dry_run: bool,
) -> Result<Outcome> {
    let project = ctx.service::<Arc<DbtProject>>()?;
    let mut requests = tables
        .iter()
        .map(|t| AddRequest::parse(t))
        .collect::<Result<Vec<_>, _>>()?;
    if missing {
        let done = parse(ctx, &project, true)?;
        let found = undefined(&done);
        if found.is_empty() {
            if done.success() {
                println!("dbt reports no undefined sources");
            } else {
                print_dbt_errors(&done);
                bail!("dbt parse failed, but not because of undefined sources");
            }
        }
        for request in found {
            if !requests.contains(&request) {
                println!("dbt reports undefined source {request}");
                requests.push(request);
            }
        }
    }
    if requests.is_empty() {
        if missing {
            return Ok(Outcome::Success);
        }
        bail!("nothing to add: give database.table arguments or --missing");
    }

    let catalog = catalog(ctx, settings)?;
    let results = vdp_sources::add(
        &project.root,
        &project.model_paths,
        settings,
        &catalog,
        &requests,
        dry_run,
    )?;
    let verb = if dry_run { "would add" } else { "added" };
    let mut ok = true;
    for (request, outcome) in &results {
        match outcome {
            AddOutcome::Added { file, columns } => println!(
                "{verb} {request} ({columns} columns) to {}",
                relative(&project.root, file)
            ),
            AddOutcome::Refreshed { file } => {
                println!(
                    "refreshed the columns of {request} in {}",
                    relative(&project.root, file)
                );
            }
            AddOutcome::AlreadyDeclared { file } => {
                println!(
                    "{request} is already declared in {}",
                    relative(&project.root, file)
                );
            }
            AddOutcome::NotInDatabase {
                database,
                suggestions,
            } => {
                ok = false;
                let hint = if suggestions.is_empty() {
                    String::new()
                } else {
                    format!("; did you mean {}?", suggestions.join(", "))
                };
                println!(
                    "error: database `{database}` has no table `{}`{hint}",
                    request.table
                );
            }
            AddOutcome::Unsupported { file, reason } => {
                ok = false;
                println!(
                    "error: {request}: {} ({reason})",
                    relative(&project.root, file)
                );
            }
        }
    }
    if missing
        && !dry_run
        && results
            .iter()
            .any(|(_, o)| matches!(o, AddOutcome::Added { .. }))
    {
        let done = parse(ctx, &project, true)?;
        let still = undefined(&done);
        if still.is_empty() {
            println!("dbt parse: no undefined sources left");
        } else {
            ok = false;
            let names: Vec<String> = still.iter().map(ToString::to_string).collect();
            println!("dbt still reports undefined sources: {}", names.join(", "));
        }
    }
    Ok(Outcome::from_success(ok))
}

fn rewrite(
    ctx: &Context,
    settings: &SourcesSettings,
    databases: &[String],
    no_trim: bool,
) -> Result<Outcome> {
    let project = ctx.service::<Arc<DbtProject>>()?;
    let catalog = catalog(ctx, settings)?;
    let report = vdp_sources::rewrite::refresh(
        &project.root,
        &project.model_paths,
        settings,
        &catalog,
        databases,
    )?;
    for entry in &report {
        match entry {
            Refreshed::Written {
                database,
                file,
                tables,
            } => println!(
                "{database}: {tables} tables into {}",
                relative(&project.root, file)
            ),
            Refreshed::SkippedHandWritten { database, file } => println!(
                "{database}: skipped, declared by hand in {}",
                relative(&project.root, file)
            ),
            Refreshed::Empty { database } => println!("{database}: no tables (does it exist?)"),
        }
    }
    if no_trim {
        return Ok(Outcome::Success);
    }
    let done = parse(ctx, &project, false)?;
    if !done.success() {
        print_dbt_errors(&done);
        bail!("dbt parse failed; the regenerated files are kept untrimmed");
    }
    let manifest = Manifest::load(&project.manifest_path()).context("reading the manifest")?;
    for entry in vdp_sources::rewrite::trim(&project.root, settings, &manifest, databases)? {
        match entry {
            Trimmed::Kept { file, tables } => {
                println!(
                    "kept {tables} referenced tables in {}",
                    relative(&project.root, &file)
                );
            }
            Trimmed::Deleted { file } => {
                println!(
                    "deleted {} (nothing referenced)",
                    relative(&project.root, &file)
                );
            }
        }
    }
    Ok(Outcome::Success)
}
