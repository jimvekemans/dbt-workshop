use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use crate::history::NoHistory;
use crate::ledger::{LedgerError, LedgerWriter};
use chrono::{DateTime, Utc};
use thiserror::Error;
use vdp_vaultspeed_domain::{
    AlterOp, ChangeEvent, ChangeOutcome, ChangeRecorder, Decision, GroupReport, LedgerEntry,
    LedgerOutcome, ObjectKey, PlannedGroup, RunId, SkipReason, SourcePlan, Statement,
    StatementKind, StatementOutcome, StatementReport,
};

use crate::executor::{ErrorClassifier, ErrorVerdict, StatementExecutor};

/// What to do when a group fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    /// Keep going with the next group (Python parity for DDL/ETL).
    Continue,
    /// Skip the rest of this source, continue with the next source.
    HaltSource,
    /// Stop the whole run (default for PML: generations depend on its objects).
    HaltRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltScope {
    Source,
    Run,
}

/// Why processing stopped early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Halt {
    pub scope: HaltScope,
    pub reason: String,
}

impl fmt::Display for Halt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scope = match self.scope {
            HaltScope::Source => "source",
            HaltScope::Run => "run",
        };
        write!(f, "{scope} halted: {}", self.reason)
    }
}

/// Fatal runner errors. Statement failures are not errors: they are reported.
#[derive(Debug, Error)]
pub enum ExecError {
    #[error("cannot record to the ledger, stopping to avoid unrecorded changes: {0}")]
    Ledger(#[from] LedgerError),
}

/// Identity of the run, stamped onto ledger entries.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub run_id: RunId,
    /// The environment name as configured (`LDV`, `dev`, ...), stamped onto change events.
    pub environment: String,
    pub git_sha: Option<String>,
}

/// Result of running one source plan.
#[derive(Debug, Clone)]
pub struct SourceRun {
    pub groups: Vec<GroupReport>,
    pub halt: Option<Halt>,
}

/// Executes plans, recording every outcome in the ledger as it happens.
#[derive(Debug, Clone)]
pub struct PlanRunner {
    executor: Arc<dyn StatementExecutor>,
    classifier: Arc<dyn ErrorClassifier>,
    ledger: Arc<dyn LedgerWriter>,
    history: Arc<dyn ChangeRecorder>,
    skip_dependents: bool,
}

/// What happened to one statement, as far as the object history is concerned.
struct Facts {
    outcome: ChangeOutcome,
    rows_affected: Option<i64>,
    error_code: Option<u16>,
    error_message: Option<String>,
    ledger_sequence: Option<u64>,
    run_time: DateTime<Utc>,
    duration_ms: Option<u64>,
}

impl Facts {
    /// A statement that did not run: the time is the moment the decision was acted on.
    fn idle(outcome: ChangeOutcome) -> Self {
        Self {
            outcome,
            rows_affected: None,
            error_code: None,
            error_message: None,
            ledger_sequence: None,
            run_time: Utc::now(),
            duration_ms: None,
        }
    }
}

/// Where in the plan a statement sits; everything a change event needs besides the statement itself.
struct Site<'a> {
    planned: &'a PlannedGroup,
    plan: &'a SourcePlan,
    ctx: &'a RunContext,
    phase: &'a str,
    decision: &'a str,
}

impl PlanRunner {
    /// With `skip_dependents`, groups touching an object whose creation failed earlier in
    /// the same source are skipped instead of producing a cascade of errors.
    pub fn new(
        executor: Arc<dyn StatementExecutor>,
        classifier: Arc<dyn ErrorClassifier>,
        ledger: Arc<dyn LedgerWriter>,
        skip_dependents: bool,
    ) -> Self {
        Self {
            executor,
            classifier,
            ledger,
            history: Arc::new(NoHistory),
            skip_dependents,
        }
    }

    /// Reports every statement of every group to `history` (the object history). Without it nothing is
    /// reported. A failing recorder is logged and never fails the run: the history is an audit record, the
    /// ledger decides what runs.
    #[must_use]
    pub fn with_history(mut self, history: Arc<dyn ChangeRecorder>) -> Self {
        self.history = history;
        self
    }

    pub fn run(
        &self,
        plan: &SourcePlan,
        ctx: &RunContext,
        policy: FailurePolicy,
    ) -> Result<SourceRun, ExecError> {
        let mut reports = Vec::new();
        let mut halt: Option<Halt> = None;
        let mut failed_objects: HashSet<String> = HashSet::new();

        for (phase, planned) in plan.groups() {
            let decision = if halt.is_some() {
                Decision::Skip(SkipReason::Halted)
            } else {
                self.effective_decision(planned, &failed_objects)
            };
            let shown = decision.to_string();
            let site = Site {
                planned,
                plan,
                ctx,
                phase: &phase.name,
                decision: &shown,
            };
            let statements = match &decision {
                Decision::Execute(_) => self.execute_group(&site)?,
                Decision::Skip(SkipReason::Satisfied { evidence }) => {
                    self.record_satisfied(&site, evidence)?
                }
                Decision::Skip(_) => {
                    self.idle_group(&site, ChangeOutcome::Skipped);
                    not_run(&planned.group.statements)
                }
                // Nothing runs and nothing is recorded; the group fails so a person looks.
                Decision::Refuse { reason } => {
                    self.idle_group(&site, ChangeOutcome::Refused);
                    refused(&planned.group.statements, reason)
                }
            };
            let report = GroupReport {
                group_id: planned.group.id.to_string(),
                artifact: planned.group.info.artifact,
                job: planned.group.info.job.to_string(),
                rel_path: planned.group.info.rel_path.to_string(),
                phase: phase.name.clone(),
                decision,
                statements,
            };
            if report.failed() {
                for statement in planned
                    .group
                    .statements
                    .iter()
                    .filter(|s| s.kind.is_create())
                {
                    if let Some(target) = &statement.target {
                        failed_objects.insert(target.qualified_name());
                    }
                }
                if halt.is_none() {
                    halt = halt_for(policy, &report);
                }
            }
            reports.push(report);
        }
        Ok(SourceRun {
            groups: reports,
            halt,
        })
    }

    fn effective_decision(
        &self,
        planned: &PlannedGroup,
        failed_objects: &HashSet<String>,
    ) -> Decision {
        if self.skip_dependents
            && planned.decision.is_execute()
            && let Some(object) = planned
                .group
                .targets()
                .map(vdp_vaultspeed_domain::ObjectRef::qualified_name)
                .find(|name| failed_objects.contains(name))
        {
            return Decision::Skip(SkipReason::UpstreamFailed { object });
        }
        planned.decision.clone()
    }

    fn execute_group(&self, site: &Site<'_>) -> Result<Vec<StatementReport>, ExecError> {
        let planned = site.planned;
        let mut reports = Vec::with_capacity(planned.group.statements.len());
        let mut failed = false;
        for (index, statement) in planned.group.statements.iter().enumerate() {
            if failed {
                self.emit(site, index, statement, Facts::idle(ChangeOutcome::NotRun));
                reports.push(report(statement, StatementOutcome::NotRun));
                continue;
            }
            let run_time = Utc::now();
            let timer = Instant::now();
            let result = self.executor.execute(&statement.effective);
            let duration_ms = u64::try_from(timer.elapsed().as_millis()).unwrap_or(u64::MAX);
            let mut facts = Facts {
                outcome: ChangeOutcome::Applied,
                rows_affected: None,
                error_code: None,
                error_message: None,
                ledger_sequence: None,
                run_time,
                duration_ms: Some(duration_ms),
            };
            let outcome = match result {
                Ok(rows) => {
                    facts.rows_affected = rows;
                    StatementOutcome::Applied
                }
                Err(error) => {
                    facts.error_code = error.code;
                    facts.error_message = Some(error.message.clone());
                    match self.classifier.classify(statement, &error) {
                        ErrorVerdict::Satisfied(evidence) => {
                            facts.outcome = ChangeOutcome::Satisfied;
                            StatementOutcome::Satisfied { evidence }
                        }
                        ErrorVerdict::Failure => {
                            failed = true;
                            facts.outcome = ChangeOutcome::Failed;
                            tracing::error!(group = %planned.group.id, %error, "statement failed");
                            StatementOutcome::Failed {
                                code: error.code,
                                message: error.message,
                            }
                        }
                    }
                }
            };
            let recorded = self.record(site, statement, &outcome);
            facts.ledger_sequence = recorded.as_ref().ok().copied().flatten();
            self.emit(site, index, statement, facts);
            recorded?;
            reports.push(report(statement, outcome));
        }
        Ok(reports)
    }

    fn record_satisfied(
        &self,
        site: &Site<'_>,
        evidence: &vdp_vaultspeed_domain::Evidence,
    ) -> Result<Vec<StatementReport>, ExecError> {
        let outcome = StatementOutcome::Satisfied {
            evidence: evidence.clone(),
        };
        let mut reports = Vec::new();
        for (index, statement) in site.planned.group.statements.iter().enumerate() {
            let mut facts = Facts::idle(ChangeOutcome::Satisfied);
            let recorded = self.record(site, statement, &outcome);
            facts.ledger_sequence = recorded.as_ref().ok().copied().flatten();
            self.emit(site, index, statement, facts);
            recorded?;
            reports.push(report(statement, outcome.clone()));
        }
        Ok(reports)
    }

    /// Reports the statements of a group that did not run (skipped, refused) to the history.
    fn idle_group(&self, site: &Site<'_>, outcome: ChangeOutcome) {
        for (index, statement) in site.planned.group.statements.iter().enumerate() {
            self.emit(site, index, statement, Facts::idle(outcome));
        }
    }

    /// Records the statement in the ledger; returns the sequence of the entry (`None` when nothing was
    /// recorded).
    fn record(
        &self,
        site: &Site<'_>,
        statement: &Statement,
        outcome: &StatementOutcome,
    ) -> Result<Option<u64>, ExecError> {
        let (outcome, detail) = match outcome {
            StatementOutcome::Applied => (LedgerOutcome::Applied, None),
            StatementOutcome::Satisfied { evidence } => {
                (LedgerOutcome::Satisfied, Some(evidence.to_string()))
            }
            StatementOutcome::Failed { code, message } => (
                LedgerOutcome::Failed,
                Some(code.map_or_else(|| message.clone(), |c| format!("({c}) {message}"))),
            ),
            StatementOutcome::NotRun => return Ok(None),
        };
        let recorded = self.ledger.record(LedgerEntry {
            sequence: 0,
            run_id: site.ctx.run_id.clone(),
            recorded_at: Utc::now(),
            source: site.plan.source.label().to_owned(),
            job: site.planned.group.info.job.to_string(),
            rel_path: rel_path(site.planned),
            object_key: statement.object_key().map(|k| k.to_string()),
            statement_kind: statement.kind.label(),
            raw_fp: Some(statement.raw_fp),
            effective_fp: statement.effective_fp,
            outcome,
            detail,
            git_sha: site.ctx.git_sha.clone(),
            renamed_to: renamed_to(statement),
        })?;
        Ok(Some(recorded.sequence))
    }

    fn emit(&self, site: &Site<'_>, index: usize, statement: &Statement, facts: Facts) {
        let event = ChangeEvent {
            run_id: site.ctx.run_id.clone(),
            environment: site.ctx.environment.clone(),
            source: site.plan.source.label().to_owned(),
            job: site.planned.group.info.job.to_string(),
            rel_path: rel_path(site.planned),
            group_id: site.planned.group.id.to_string(),
            phase: site.phase.to_owned(),
            statement_index: u32::try_from(index).unwrap_or(u32::MAX),
            kind: statement.kind.clone(),
            target: statement.target.clone(),
            raw_sql: statement.raw.clone(),
            effective_sql: statement.effective.clone(),
            raw_fp: statement.raw_fp,
            effective_fp: statement.effective_fp,
            applied_rules: statement.applied_rules.clone(),
            decision: site.decision.to_owned(),
            outcome: facts.outcome,
            rows_affected: facts.rows_affected,
            error_code: facts.error_code,
            error_message: facts.error_message,
            ledger_sequence: facts.ledger_sequence,
            run_time: facts.run_time,
            duration_ms: facts.duration_ms,
            git_sha: site.ctx.git_sha.clone(),
        };
        if let Err(error) = self.history.record(&event) {
            tracing::warn!(%error, group = %site.planned.group.id, "object history could not record a change");
        }
    }
}

/// `<artifact>/<path>`, as the ledger keeps it.
fn rel_path(planned: &PlannedGroup) -> String {
    format!(
        "{}/{}",
        planned.group.info.artifact, planned.group.info.rel_path
    )
}

/// The definition key of a rename's new name.
fn renamed_to(statement: &Statement) -> Option<String> {
    match &statement.kind {
        StatementKind::Alter {
            op: AlterOp::Rename { to },
        } => Some(ObjectKey::definition(to.clone()).to_string()),
        _ => None,
    }
}

fn halt_for(policy: FailurePolicy, report: &GroupReport) -> Option<Halt> {
    let scope = match policy {
        FailurePolicy::Continue => return None,
        FailurePolicy::HaltSource => HaltScope::Source,
        FailurePolicy::HaltRun => HaltScope::Run,
    };
    Some(Halt {
        scope,
        reason: format!("{} ({}) failed", report.job, report.rel_path),
    })
}

fn report(statement: &Statement, outcome: StatementOutcome) -> StatementReport {
    StatementReport {
        kind: statement.kind.label(),
        target: statement
            .target
            .as_ref()
            .map(vdp_vaultspeed_domain::ObjectRef::qualified_name),
        raw_fp: statement.raw_fp,
        effective_sql: statement.effective.clone(),
        outcome,
    }
}

fn refused(statements: &[Statement], reason: &str) -> Vec<StatementReport> {
    statements
        .iter()
        .enumerate()
        .map(|(index, s)| {
            let outcome = if index == 0 {
                StatementOutcome::Failed {
                    code: None,
                    message: format!("refused by planner: {reason}"),
                }
            } else {
                StatementOutcome::NotRun
            };
            report(s, outcome)
        })
        .collect()
}

fn not_run(statements: &[Statement]) -> Vec<StatementReport> {
    statements
        .iter()
        .map(|s| report(s, StatementOutcome::NotRun))
        .collect()
}

#[cfg(test)]
mod tests {
    use vdp_db::DbError;
    use vdp_vaultspeed_domain::{
        ArtifactKind, GroupId, JobInfo, JobName, Layer, OrderKey, PlanPhase, RelPath, SourceKind,
        StatementGroup,
    };
    use vdp_vaultspeed_sql::{ArtifactPipeline, NoRewrite};

    use super::*;
    use crate::executor::NoClassification;
    use crate::history::MemoryHistory;
    use crate::ledger::{LedgerReader, MemoryLedger};

    /// Fails statements containing `BOOM`, reports 3 rows for inserts and nothing otherwise.
    #[derive(Debug)]
    struct Fake;

    impl StatementExecutor for Fake {
        fn execute(&self, sql: &str) -> Result<Option<i64>, DbError> {
            if sql.contains("BOOM") {
                Err(DbError {
                    code: Some(1064),
                    message: "syntax".into(),
                })
            } else if sql.starts_with("INSERT") {
                Ok(Some(3))
            } else {
                Ok(None)
            }
        }
    }

    fn group(sql: &str, ordinal: usize) -> StatementGroup {
        let statements = ArtifactPipeline::new(
            Arc::new(vdp_vaultspeed_sql::WholeFileSplitter),
            vec![],
            Arc::new(NoRewrite),
        )
        .statements(sql)
        .unwrap();
        StatementGroup {
            id: GroupId::new("PML", &RelPath::new("t.sql"), ordinal),
            info: JobInfo {
                artifact: ArtifactKind::Ddl,
                job: JobName::from_file_stem("T"),
                rel_path: RelPath::new("t.sql"),
                layer: Layer::NotApplicable,
                order: OrderKey::new(vec![1]),
            },
            ordinal,
            statements,
        }
    }

    #[test]
    fn every_statement_of_every_group_becomes_one_event() {
        let ledger = Arc::new(MemoryLedger::new());
        let history = Arc::new(MemoryHistory::new());
        let runner = PlanRunner::new(
            Arc::new(Fake),
            Arc::new(NoClassification),
            ledger.clone(),
            false,
        )
        .with_history(history.clone());

        // WholeFileSplitter keeps a script as one statement, so every group is built from one-statement
        // scripts joined by hand.
        let mut running = group("INSERT INTO s.t SELECT 1", 0);
        running
            .statements
            .extend(group("SELECT BOOM FROM s.t", 0).statements);
        running
            .statements
            .extend(group("INSERT INTO s.t SELECT 2", 0).statements);
        let planned = vec![
            PlannedGroup {
                group: running,
                decision: Decision::Execute(vdp_vaultspeed_domain::ExecuteReason::New),
            },
            PlannedGroup {
                group: group("CREATE TABLE s.u (a INT)", 1),
                decision: Decision::Skip(SkipReason::AlreadyApplied { sequence: 9 }),
            },
            PlannedGroup {
                group: group("CREATE TABLE s.v (a INT)", 2),
                decision: Decision::Refuse {
                    reason: "no".into(),
                },
            },
        ];
        let plan = SourcePlan {
            source: SourceKind::Pml,
            phases: vec![PlanPhase {
                name: "phase one".into(),
                groups: planned,
            }],
        };
        let ctx = RunContext {
            run_id: RunId::from_string("r1"),
            environment: "LDV".into(),
            git_sha: Some("abc".into()),
        };
        let run = runner.run(&plan, &ctx, FailurePolicy::Continue).unwrap();
        assert_eq!(run.groups.len(), 3);

        let events = history.events();
        let outcomes: Vec<_> = events.iter().map(|e| e.outcome).collect();
        assert_eq!(
            outcomes,
            vec![
                ChangeOutcome::Applied,
                ChangeOutcome::Failed,
                ChangeOutcome::NotRun,
                ChangeOutcome::Skipped,
                ChangeOutcome::Refused,
            ]
        );

        let applied = &events[0];
        assert_eq!(applied.rows_affected, Some(3));
        assert_eq!(applied.ledger_sequence, Some(1));
        assert!(applied.duration_ms.is_some());
        assert_eq!(applied.environment, "LDV");
        assert_eq!(applied.phase, "phase one");
        assert_eq!(applied.statement_index, 0);
        assert_eq!(applied.decision, "execute: new");
        assert_eq!(applied.git_sha.as_deref(), Some("abc"));
        assert_eq!(applied.target.as_ref().unwrap().qualified_name(), "s.t");

        let failed = &events[1];
        assert_eq!(failed.error_code, Some(1064));
        assert_eq!(failed.error_message.as_deref(), Some("syntax"));
        assert_eq!(
            failed.ledger_sequence,
            Some(2),
            "failures are recorded for audit"
        );
        assert_eq!(failed.statement_index, 1);

        let not_run = &events[2];
        assert_eq!(not_run.ledger_sequence, None);
        assert_eq!(not_run.duration_ms, None);
        assert_eq!(not_run.statement_index, 2);

        let skipped = &events[3];
        assert_eq!(skipped.decision, "skip: already applied (ledger #9)");
        assert_eq!((skipped.ledger_sequence, skipped.duration_ms), (None, None));

        let refused = &events[4];
        assert_eq!(refused.decision, "refused: no");
        assert_eq!(refused.group_id, "PML:t.sql#2");

        // The ledger saw exactly the statements that ran.
        assert_eq!(ledger.entries().unwrap().len(), 2);
    }

    #[test]
    fn a_failing_recorder_never_fails_the_run() {
        #[derive(Debug)]
        struct Broken;
        impl ChangeRecorder for Broken {
            fn record(&self, _: &ChangeEvent) -> Result<(), vdp_vaultspeed_domain::HistoryError> {
                Err(vdp_vaultspeed_domain::HistoryError::new("disk full"))
            }
        }
        let runner = PlanRunner::new(
            Arc::new(Fake),
            Arc::new(NoClassification),
            Arc::new(MemoryLedger::new()),
            false,
        )
        .with_history(Arc::new(Broken));
        let plan = SourcePlan {
            source: SourceKind::Pml,
            phases: vec![PlanPhase {
                name: "p".into(),
                groups: vec![PlannedGroup {
                    group: group("INSERT INTO s.t SELECT 1", 0),
                    decision: Decision::Execute(vdp_vaultspeed_domain::ExecuteReason::New),
                }],
            }],
        };
        let ctx = RunContext {
            run_id: RunId::from_string("r"),
            environment: "LDV".into(),
            git_sha: None,
        };
        let run = runner.run(&plan, &ctx, FailurePolicy::HaltRun).unwrap();
        assert!(!run.groups[0].failed());
    }
}
