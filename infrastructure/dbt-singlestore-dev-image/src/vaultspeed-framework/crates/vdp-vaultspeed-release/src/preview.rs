//! What CI would deploy after a merge: the engine planned offline on the merged work tree, against the ledger the
//! environment branch carries (its JSONL mirror, or its legacy `vaultspeed/deployed` state imported in memory).
//! No database, no Vault, nothing written.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed::ledger::{JsonlLedger, Ledger, LegacyImporter, MemoryLedger};
use vdp_vaultspeed::report::{ConsoleSink, ReportSink};
use vdp_vaultspeed::wiring::{self, Infrastructure, Project, RunOptions};
use vdp_vaultspeed_domain::{Decision, Environment, RunId, RunMode, RunReport, StatementOutcome};

use crate::VsError;

/// One staged source in the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SourcePreview {
    pub source: String,
    /// Groups that would run.
    pub execute: usize,
    /// Groups already in effect, excluded or duplicates.
    pub skip: usize,
    pub dags: usize,
    /// Refusals, halts and errors: the deploy would fail on these.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PlanPreview {
    pub environment: String,
    /// `jsonl` (the ledger mirror) or `legacy` (imported from `vaultspeed/deployed`).
    pub ledger: String,
    pub sources: Vec<SourcePreview>,
    /// What `vdp vaultspeed plan` prints.
    pub text: String,
}

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn plan_error(e: impl std::fmt::Display) -> VsError {
    VsError::Plan(e.to_string())
}

/// The ledger the environment branch carries: its JSONL mirror, else its legacy `deployed/` state imported in
/// memory. Keys are qualified with the environment's database, as `wiring::ledger` does, so legacy unqualified
/// keys match.
fn preview_ledger(
    project: &Project,
    environment: Environment,
) -> Result<(Arc<dyn Ledger>, &'static str), VsError> {
    let jsonl = project.jsonl_ledger_path(environment);
    if jsonl.is_file() {
        let database = &project.config.environment(environment).database;
        let ledger = JsonlLedger::snapshot_in(&jsonl, database).map_err(plan_error)?;
        return Ok((Arc::new(ledger), "jsonl"));
    }
    let memory = Arc::new(MemoryLedger::new());
    LegacyImporter::default()
        .import(
            &project.layout.deployed_dir(),
            &RunId::from_string("legacy-preview"),
            memory.as_ref(),
        )
        .map_err(plan_error)?;
    Ok((memory, "legacy"))
}

/// Plans what is staged in `work_tree` for `environment`.
pub fn plan_preview(
    work_tree: &Path,
    settings: &VaultspeedSettings,
    environment: Environment,
) -> Result<PlanPreview, VsError> {
    let project = Project::with_config(work_tree, settings.clone()).map_err(plan_error)?;
    let (ledger, kind) = preview_ledger(&project, environment)?;
    let infra = Infrastructure {
        session: None,
        ledger,
        singlestore_host: wiring::placeholder_host(environment, None),
        airflow: None,
    };
    let options = RunOptions {
        environment,
        mode: RunMode::Preview,
        only: None,
        git_sha: None,
    };
    let report: RunReport = wiring::engine(&project, &options, &infra)
        .map_err(plan_error)?
        .run()
        .map_err(plan_error)?;
    let buffer = Buffer::default();
    ConsoleSink::new(false, Box::new(buffer.clone()))
        .write(&report)
        .map_err(plan_error)?;
    let text = String::from_utf8_lossy(
        &buffer
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
    .into_owned();
    let sources = report
        .sources
        .iter()
        .map(|source| {
            let execute = source
                .groups
                .iter()
                .filter(|g| g.decision.is_execute())
                .count();
            // One line per distinct problem (a refusal, or a failed statement), with how often it occurs.
            let mut counted: Vec<(String, usize)> = Vec::new();
            let mut add = |line: String| match counted.iter_mut().find(|(l, _)| *l == line) {
                Some((_, n)) => *n += 1,
                None => counted.push((line, 1)),
            };
            for group in &source.groups {
                if let Decision::Refuse { reason } = &group.decision {
                    add(format!("{}: refused: {reason}", group.rel_path));
                } else if let Some(message) =
                    group.statements.iter().find_map(|s| match &s.outcome {
                        StatementOutcome::Failed { message, .. } => Some(message),
                        _ => None,
                    })
                {
                    add(format!("{}: {message}", group.rel_path));
                }
            }
            source.halted.iter().cloned().for_each(&mut add);
            source.errors.iter().cloned().for_each(&mut add);
            let problems = counted
                .into_iter()
                .map(|(line, n)| {
                    if n > 1 {
                        format!("{line} (×{n})")
                    } else {
                        line
                    }
                })
                .collect();
            SourcePreview {
                source: source.source.clone(),
                execute,
                skip: source.groups.len() - execute,
                dags: source.dags.len(),
                problems,
            }
        })
        .collect();
    Ok(PlanPreview {
        environment: environment.to_string(),
        ledger: kind.into(),
        sources,
        text,
    })
}
