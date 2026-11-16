use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use vdp_vaultspeed_domain::{Decision, RunReport, SkipReason, StatementOutcome};

use crate::report::{ReportError, ReportSink};

/// Human-readable summary. With `explain`, every group's decision is listed and DAG
/// diffs are printed.
pub struct ConsoleSink {
    explain: bool,
    out: Mutex<Box<dyn Write + Send>>,
}

impl ConsoleSink {
    pub fn stdout(explain: bool) -> Self {
        Self::new(explain, Box::new(io::stdout()))
    }

    pub fn new(explain: bool, out: Box<dyn Write + Send>) -> Self {
        Self {
            explain,
            out: Mutex::new(out),
        }
    }

    fn render(&self, report: &RunReport) -> String {
        let mut text = String::new();
        let mode = if report.mode.is_preview() {
            "PREVIEW (nothing was changed)"
        } else {
            "APPLY"
        };
        let _ = writeln!(
            text,
            "VSAF run {} on {} - {mode}",
            report.run_id, report.environment
        );
        if report.sources.is_empty() {
            text.push_str("  nothing staged: no PML scripts and no generation zips\n");
        }
        for source in &report.sources {
            let _ = write!(text, "\n== {}\n", source.source);
            let mut skips: BTreeMap<&str, usize> = BTreeMap::new();
            let (mut executed, mut failed, mut refused) = (0, 0, 0);
            for group in &source.groups {
                match &group.decision {
                    Decision::Execute(_) if group.failed() => failed += 1,
                    Decision::Execute(_) => executed += 1,
                    Decision::Skip(reason) => *skips.entry(skip_label(reason)).or_default() += 1,
                    Decision::Refuse { .. } => refused += 1,
                }
                if self.explain {
                    let _ = writeln!(
                        text,
                        "  [{}] {} - {}",
                        group.phase, group.group_id, group.decision
                    );
                }
            }
            let _ = write!(
                text,
                "  executed: {executed}, failed: {failed}, refused: {refused}"
            );
            for (label, count) in &skips {
                let _ = write!(text, ", {label}: {count}");
            }
            text.push('\n');
            for group in source.groups.iter().filter(|g| g.failed()) {
                for statement in &group.statements {
                    if let StatementOutcome::Failed { code, message } = &statement.outcome {
                        let code = code.map(|c| format!("({c}) ")).unwrap_or_default();
                        let _ = writeln!(
                            text,
                            "  FAILED {} [{}]: {code}{message}",
                            group.job, statement.kind
                        );
                    }
                }
            }
            for dag in &source.dags {
                let status = match (&dag.error, dag.published) {
                    (Some(error), _) => format!("FAILED: {error}"),
                    (None, true) => format!("published ({} changes)", dag.transformations.len()),
                    (None, false) => format!("rendered ({} changes)", dag.transformations.len()),
                };
                let _ = writeln!(
                    text,
                    "  DAG {} -> {}: {status}",
                    dag.source_file, dag.target_file
                );
                if self.explain
                    && let Some(diff) = &dag.diff
                {
                    text.push_str(diff);
                }
            }
            for error in &source.errors {
                let _ = writeln!(text, "  ERROR {error}");
            }
            if let Some(halt) = &source.halted {
                let _ = writeln!(text, "  HALTED {halt}");
            }
            for note in &source.housekeeping {
                let _ = writeln!(text, "  - {note}");
            }
        }
        let totals = report.totals();
        let _ = write!(
            text,
            "\nTotal: {} applied, {} satisfied, {} failed statements; {} groups executed, {} skipped; {} DAGs published\n",
            totals.applied,
            totals.satisfied,
            totals.failed,
            totals.executed_groups,
            totals.skipped_groups,
            totals.dags
        );
        text
    }
}

impl std::fmt::Debug for ConsoleSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleSink")
            .field("explain", &self.explain)
            .finish_non_exhaustive()
    }
}

fn skip_label(reason: &SkipReason) -> &'static str {
    match reason {
        SkipReason::AlreadyApplied { .. } => "already applied",
        SkipReason::Satisfied { .. } => "already in effect",
        SkipReason::Excluded { .. } => "excluded",
        SkipReason::UpstreamFailed { .. } => "upstream failed",
        SkipReason::Halted => "halted",
        SkipReason::DuplicateInPlan { .. } => "duplicate in plan",
    }
}

impl ReportSink for ConsoleSink {
    fn write(&self, report: &RunReport) -> Result<Vec<PathBuf>, ReportError> {
        let text = self.render(report);
        let mut out = self
            .out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.write_all(text.as_bytes())
            .and_then(|()| out.flush())
            .map_err(|source| ReportError::Io {
                path: PathBuf::from("<stdout>"),
                source,
            })?;
        Ok(Vec::new())
    }
}
