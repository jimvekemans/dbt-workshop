//! `vaultspeed history object|objects`: reads the DuckDB object history.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use vdp_core::{Context, Outcome};
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed_history::{DuckDbHistory, ObjectRow, ObjectStatus, StatementRow};

use crate::args::{StatusFilter, VaultspeedHistoryArgs, VaultspeedHistoryCommand};

/// The history file: `--path`, else `[vaultspeed.history].path` in the framework checkout (`--framework-dir`,
/// else `[vaultspeed].project_dir`), relative to the current directory.
pub fn history_path(
    ctx: &Context,
    framework_dir: Option<&Path>,
    explicit: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(ctx.cwd().join(path));
    }
    let settings: VaultspeedSettings = ctx.config().section(VaultspeedSettings::SECTION)?;
    let root = ctx
        .cwd()
        .join(framework_dir.unwrap_or(&settings.project_dir));
    Ok(root.join(&settings.history.path))
}

pub fn run(
    ctx: &Context,
    framework_dir: Option<&Path>,
    args: &VaultspeedHistoryArgs,
) -> Result<Outcome> {
    let path = history_path(ctx, framework_dir, args.path.as_deref())?;
    if !path.is_file() {
        bail!(
            "no object history at {} (it is written by applying `vaultspeed deploy` runs; --path names another file)",
            path.display()
        );
    }
    let history = DuckDbHistory::open(&path)
        .with_context(|| format!("opening the object history {}", path.display()))?;
    let environment = ctx.environment().to_string();
    match &args.command {
        VaultspeedHistoryCommand::Object { identifier, all } => {
            object(&history, &environment, identifier, *all, args.json)
        }
        VaultspeedHistoryCommand::Objects { status } => {
            objects(&history, &environment, *status, args.json)
        }
    }
}

fn object(
    history: &DuckDbHistory,
    environment: &str,
    identifier: &str,
    all: bool,
    json: bool,
) -> Result<Outcome> {
    if all {
        let lifespans = history.lifespans(environment, identifier)?;
        print_objects(&lifespans, json)?;
        return Ok(found(!lifespans.is_empty(), environment, identifier));
    }
    let statements = history.lifespan(environment, identifier)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&statements)?);
    } else if !statements.is_empty() {
        print!("{}", statements_table(&statements));
    }
    Ok(found(!statements.is_empty(), environment, identifier))
}

fn found(any: bool, environment: &str, identifier: &str) -> Outcome {
    if any {
        return Outcome::Success;
    }
    eprintln!("no object named {identifier} in the {environment} history");
    Outcome::Failures
}

fn objects(
    history: &DuckDbHistory,
    environment: &str,
    status: Option<StatusFilter>,
    json: bool,
) -> Result<Outcome> {
    let status = status.map(|s| match s {
        StatusFilter::Live => ObjectStatus::Live,
        StatusFilter::Dropped => ObjectStatus::Dropped,
    });
    print_objects(&history.objects(environment, status)?, json)?;
    Ok(Outcome::Success)
}

fn print_objects(objects: &[ObjectRow], json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(objects)?);
    } else if !objects.is_empty() {
        print!("{}", objects_table(objects));
    }
    Ok(())
}

fn time(value: Option<DateTime<Utc>>) -> String {
    value.map_or_else(
        || "-".to_owned(),
        |t| t.to_rfc3339_opts(SecondsFormat::Secs, true),
    )
}

/// Left-aligned columns, each as wide as its widest cell.
fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..header.len())
        .map(|i| {
            rows.iter()
                .map(|r| r[i].chars().count())
                .chain([header[i].len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let mut out = String::new();
    let mut line = |cells: &mut dyn Iterator<Item = &str>| {
        let cells: Vec<String> = cells
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        let _ = writeln!(out, "{}", cells.join("  ").trim_end());
    };
    line(&mut header.iter().copied());
    for row in rows {
        line(&mut row.iter().map(String::as_str));
    }
    out
}

fn statements_table(statements: &[StatementRow]) -> String {
    let rows: Vec<Vec<String>> = statements
        .iter()
        .map(|s| {
            vec![
                time(Some(s.run_time)),
                format!("{}/{}", s.operation, s.statement_kind),
                s.identifier.clone().unwrap_or_else(|| "-".into()),
                s.outcome.clone(),
                s.rows_affected
                    .map_or_else(|| "-".into(), |n| n.to_string()),
                s.duration_ms
                    .map_or_else(|| "-".into(), |n| format!("{n} ms")),
            ]
        })
        .collect();
    table(
        &[
            "run_time",
            "operation",
            "identifier",
            "outcome",
            "rows",
            "duration",
        ],
        &rows,
    )
}

fn objects_table(objects: &[ObjectRow]) -> String {
    let rows: Vec<Vec<String>> = objects
        .iter()
        .map(|o| {
            vec![
                o.object_id.to_string(),
                o.object_type.clone(),
                o.identifier.clone(),
                o.first_identifier.clone(),
                o.status.clone(),
                o.origin.clone(),
                time(o.created_at),
                time(o.ended_at),
            ]
        })
        .collect();
    table(
        &[
            "id",
            "type",
            "identifier",
            "first_identifier",
            "status",
            "origin",
            "created",
            "ended",
        ],
        &rows,
    )
}
