use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use vdp_core::present::{Panel, Table};
use vdp_vaultspeed_domain::{Decision, RunReport, SkipReason, SourceReport, StatementOutcome};

use crate::report::{ReportError, ReportSink};

/// Human-readable summary. With `explain`, every group's decision is listed and DAG
/// diffs are printed.
pub struct ConsoleSink {
    explain: bool,
    out: Mutex<Box<dyn Write + Send>>,
}

impl ConsoleSink {
    /// Writes to standard output; with `explain`, every decision and the DAG diffs are listed.
    pub fn stdout(explain: bool) -> Self {
        Self::new(explain, Box::new(io::stdout()))
    }

    /// Writes to `out`; with `explain`, every decision and the DAG diffs are listed.
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
        let header = format!("run {} on {} - {mode}", report.run_id, report.environment);
        text.push_str(&Panel::new("VSAF", header).render());
        if report.sources.is_empty() {
            text.push_str("  nothing staged: no PML scripts and no generation zips\n");
        }
        for source in &report.sources {
            self.render_source(source, &mut text);
        }
        let totals = report.totals();
        let summary = Table::new([
            "applied",
            "satisfied",
            "failed",
            "executed_groups",
            "skipped_groups",
            "dags",
        ])
        .row([
            totals.applied.to_string(),
            totals.satisfied.to_string(),
            totals.failed.to_string(),
            totals.executed_groups.to_string(),
            totals.skipped_groups.to_string(),
            totals.dags.to_string(),
        ])
        .render();
        text.push('\n');
        text.push_str(&summary);
        text
    }

    fn render_source(&self, source: &SourceReport, text: &mut String) {
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
                    let error_line = format!("{code}{message}");
                    let sql = statement.effective_sql.trim();
                    let title = format!("FAILED {} [{}]", group.job, statement.kind);
                    let mut panel = Panel::new(title, error_line);
                    if !sql.is_empty() {
                        panel = panel.preformatted_block(sql);
                    }
                    text.push_str(&panel.render());
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};
    use vdp_vaultspeed_domain::{
        ArtifactKind, Decision, Environment, ExecuteReason, Fingerprint, GroupReport, RunId,
        RunMode, RunReport, SkipReason, SourceReport, StatementOutcome, StatementReport,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn renders_header_and_empty_totals_with_panel_and_table() {
        let capture = Capture::default();
        let sink = ConsoleSink::new(false, Box::new(capture.clone()));
        let report = RunReport {
            run_id: RunId::from_string("run-1"),
            environment: Environment::LDV,
            mode: RunMode::Preview,
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: None,
            sources: Vec::new(),
        };
        sink.write(&report).unwrap();
        let bytes = capture
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let output = String::from_utf8(bytes).unwrap();
        let expected = concat!(
            "╭─ VSAF ───────────────────────────────────────────────────────────────────────────────╮\n",
            "│ run run-1 on LDV - PREVIEW (nothing was changed)                                     │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            "  nothing staged: no PML scripts and no generation zips\n",
            "\n",
            "applied  satisfied  failed  executed_groups  skipped_groups  dags\n",
            "0        0          0       0                0               0\n",
        );
        assert_eq!(output, expected);
    }

    #[test]
    fn renders_88_column_failure_panel_and_totals_table() {
        let sql = "CREATE TABLE edw.hub_very_long_table_name_for_wrapping_check (col_one INT, col_two VARCHAR(100), col_three DATETIME)";
        let mut src = SourceReport::new("PML");
        src.groups.push(GroupReport {
            group_id: "g1".into(),
            artifact: ArtifactKind::Pml,
            job: "01_init".into(),
            rel_path: "01_init.sql".into(),
            phase: "pml".into(),
            decision: Decision::Execute(ExecuteReason::New),
            statements: vec![StatementReport {
                kind: "CREATE TABLE".into(),
                target: Some("edw.hub_very_long_table_name_for_wrapping_check".into()),
                raw_fp: Fingerprint::of(sql),
                effective_sql: sql.into(),
                outcome: StatementOutcome::Failed {
                    code: Some(1064),
                    message: "syntax error near token".into(),
                },
            }],
        });
        src.groups.push(GroupReport {
            group_id: "g2".into(),
            artifact: ArtifactKind::Pml,
            job: "02_skip".into(),
            rel_path: "02_skip.sql".into(),
            phase: "pml".into(),
            decision: Decision::Skip(SkipReason::Excluded {
                rule: "*.skip".into(),
            }),
            statements: vec![StatementReport {
                kind: "CREATE TABLE".into(),
                target: Some("edw.skip".into()),
                raw_fp: Fingerprint::of("CREATE TABLE edw.skip (id INT)"),
                effective_sql: "CREATE TABLE edw.skip (id INT)".into(),
                outcome: StatementOutcome::NotRun,
            }],
        });
        let report = RunReport {
            run_id: RunId::from_string("run-2"),
            environment: Environment::ABT,
            mode: RunMode::Apply,
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: None,
            sources: vec![src],
        };
        let rendered = ConsoleSink::new(false, Box::new(io::sink())).render(&report);
        let expected = concat!(
            "╭─ VSAF ───────────────────────────────────────────────────────────────────────────────╮\n",
            "│ run run-2 on ABT - APPLY                                                             │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            "\n",
            "== PML\n",
            "  executed: 0, failed: 1, refused: 0, excluded: 1\n",
            "╭─ FAILED 01_init [CREATE TABLE] ──────────────────────────────────────────────────────╮\n",
            "│ (1064) syntax error near token                                                       │\n",
            "│ CREATE TABLE edw.hub_very_long_table_name_for_wrapping_check (col_one INT, col_two V │\n",
            "│ ARCHAR(100), col_three DATETIME)                                                     │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            "\n",
            "applied  satisfied  failed  executed_groups  skipped_groups  dags\n",
            "0        0          1       1                1               0\n",
        );
        assert_eq!(rendered, expected);
        for line in rendered.lines() {
            if line.starts_with('╭') || line.starts_with('│') || line.starts_with('╰') {
                assert_eq!(line.chars().count(), 88, "line not 88 cols: {line}");
            }
        }
    }

    #[test]
    fn failure_panel_ellipsizes_120_char_job_name_and_round_trips_preformatted_sql() {
        let long_job = format!("job_{}", "x".repeat(116));
        assert_eq!(long_job.len(), 120);
        let sql = concat!(
            "SELECT\n",
            "    'a   b', col_alpha, col_beta, col_gamma, col_delta, col_epsilon, col_zeta, col_eta",
        );
        let mut src = SourceReport::new("PML");
        src.groups.push(GroupReport {
            group_id: "g1".into(),
            artifact: ArtifactKind::Pml,
            job: long_job,
            rel_path: "01_proc.sql".into(),
            phase: "pml".into(),
            decision: Decision::Execute(ExecuteReason::New),
            statements: vec![StatementReport {
                kind: "CREATE PROCEDURE".into(),
                target: Some("edw.p_long".into()),
                raw_fp: Fingerprint::of(sql),
                effective_sql: sql.into(),
                outcome: StatementOutcome::Failed {
                    code: Some(1064),
                    message: "syntax error".into(),
                },
            }],
        });
        let report = RunReport {
            run_id: RunId::from_string("run-3"),
            environment: Environment::ABT,
            mode: RunMode::Apply,
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: None,
            sources: vec![src],
        };
        let rendered = ConsoleSink::new(false, Box::new(io::sink())).render(&report);
        let expected = concat!(
            "╭─ VSAF ───────────────────────────────────────────────────────────────────────────────╮\n",
            "│ run run-3 on ABT - APPLY                                                             │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            "\n",
            "== PML\n",
            "  executed: 0, failed: 1, refused: 0\n",
            "╭─ FAILED job_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx… [CREATE PROCEDURE] ╮\n",
            "│ (1064) syntax error                                                                  │\n",
            "│ SELECT                                                                               │\n",
            "│     'a   b', col_alpha, col_beta, col_gamma, col_delta, col_epsilon, col_zeta, col_e │\n",
            "│ ta                                                                                   │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            "\n",
            "applied  satisfied  failed  executed_groups  skipped_groups  dags\n",
            "0        0          1       1                0               0\n",
        );
        assert_eq!(rendered, expected);

        let panel_sql_rows: Vec<&str> = rendered
            .lines()
            .skip_while(|line| !line.starts_with("│ (1064)"))
            .skip(1)
            .take_while(|line| line.starts_with("│ "))
            .map(|line| {
                line.strip_prefix("│ ")
                    .and_then(|s| s.strip_suffix(" │"))
                    .unwrap()
            })
            .collect();
        let line1 = panel_sql_rows[0].trim_end_matches(' ');
        let line2 = format!(
            "{}{}",
            panel_sql_rows[1],
            panel_sql_rows[2].trim_end_matches(' ')
        );
        assert_eq!(format!("{line1}\n{line2}"), sql);
    }

    #[test]
    fn failure_panel_word_wraps_long_error_message_and_expands_sql_tabs_to_88_columns() {
        let sql = "SELECT\n\t'a   b',\n\tcol_one\tFROM edw.t";
        let mut src = SourceReport::new("PML");
        src.groups.push(GroupReport {
            group_id: "g1".into(),
            artifact: ArtifactKind::Pml,
            job: "01_tabbed".into(),
            rel_path: "01_tabbed.sql".into(),
            phase: "pml".into(),
            decision: Decision::Execute(ExecuteReason::New),
            statements: vec![StatementReport {
                kind: "CREATE PROCEDURE".into(),
                target: Some("edw.t".into()),
                raw_fp: Fingerprint::of(sql),
                effective_sql: sql.into(),
                outcome: StatementOutcome::Failed {
                    code: Some(1064),
                    message: "SingleStore rejected the routine definition because a referenced column does not exist in target table".into(),
                },
            }],
        });
        let report = RunReport {
            run_id: RunId::from_string("run-4"),
            environment: Environment::ABT,
            mode: RunMode::Apply,
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: None,
            sources: vec![src],
        };
        let rendered = ConsoleSink::new(false, Box::new(io::sink())).render(&report);
        let expected = concat!(
            "╭─ VSAF ───────────────────────────────────────────────────────────────────────────────╮\n",
            "│ run run-4 on ABT - APPLY                                                             │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            "\n",
            "== PML\n",
            "  executed: 0, failed: 1, refused: 0\n",
            "╭─ FAILED 01_tabbed [CREATE PROCEDURE] ────────────────────────────────────────────────╮\n",
            "│ (1064) SingleStore rejected the routine definition because a referenced column does  │\n",
            "│ not exist in target table                                                            │\n",
            "│ SELECT                                                                               │\n",
            "│     'a   b',                                                                         │\n",
            "│     col_one FROM edw.t                                                               │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            "\n",
            "applied  satisfied  failed  executed_groups  skipped_groups  dags\n",
            "0        0          1       1                0               0\n",
        );
        assert_eq!(rendered, expected);
        assert!(!rendered.contains('\t'));
        for line in rendered.lines() {
            if line.starts_with('╭') || line.starts_with('│') || line.starts_with('╰') {
                assert_eq!(line.chars().count(), 88, "line not 88 cols: {line}");
            }
        }
    }
}
