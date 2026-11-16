use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use crate::ledger::{LedgerError, LedgerWriter};
use chrono::Utc;
use thiserror::Error;
use vdp_vaultspeed_domain::{
    Decision, GroupReport, LedgerEntry, LedgerOutcome, PlannedGroup, RunId, SkipReason, SourcePlan,
    Statement, StatementOutcome, StatementReport,
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
    skip_dependents: bool,
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
            skip_dependents,
        }
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
            let statements = match &decision {
                Decision::Execute(_) => self.execute_group(planned, plan, ctx)?,
                Decision::Skip(SkipReason::Satisfied { evidence }) => {
                    self.record_satisfied(planned, plan, ctx, evidence)?
                }
                Decision::Skip(_) => not_run(&planned.group.statements),
                // Nothing runs and nothing is recorded; the group fails so a person looks.
                Decision::Refuse { reason } => refused(&planned.group.statements, reason),
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

    fn execute_group(
        &self,
        planned: &PlannedGroup,
        plan: &SourcePlan,
        ctx: &RunContext,
    ) -> Result<Vec<StatementReport>, ExecError> {
        let mut reports = Vec::with_capacity(planned.group.statements.len());
        let mut failed = false;
        for statement in &planned.group.statements {
            if failed {
                reports.push(report(statement, StatementOutcome::NotRun));
                continue;
            }
            let outcome = match self.executor.execute(&statement.effective) {
                Ok(()) => StatementOutcome::Applied,
                Err(error) => match self.classifier.classify(statement, &error) {
                    ErrorVerdict::Satisfied(evidence) => StatementOutcome::Satisfied { evidence },
                    ErrorVerdict::Failure => {
                        failed = true;
                        tracing::error!(group = %planned.group.id, %error, "statement failed");
                        StatementOutcome::Failed {
                            code: error.code,
                            message: error.message,
                        }
                    }
                },
            };
            self.record(planned, plan, ctx, statement, &outcome)?;
            reports.push(report(statement, outcome));
        }
        Ok(reports)
    }

    fn record_satisfied(
        &self,
        planned: &PlannedGroup,
        plan: &SourcePlan,
        ctx: &RunContext,
        evidence: &vdp_vaultspeed_domain::Evidence,
    ) -> Result<Vec<StatementReport>, ExecError> {
        let outcome = StatementOutcome::Satisfied {
            evidence: evidence.clone(),
        };
        let mut reports = Vec::new();
        for statement in &planned.group.statements {
            self.record(planned, plan, ctx, statement, &outcome)?;
            reports.push(report(statement, outcome.clone()));
        }
        Ok(reports)
    }

    fn record(
        &self,
        planned: &PlannedGroup,
        plan: &SourcePlan,
        ctx: &RunContext,
        statement: &Statement,
        outcome: &StatementOutcome,
    ) -> Result<(), ExecError> {
        let (outcome, detail) = match outcome {
            StatementOutcome::Applied => (LedgerOutcome::Applied, None),
            StatementOutcome::Satisfied { evidence } => {
                (LedgerOutcome::Satisfied, Some(evidence.to_string()))
            }
            StatementOutcome::Failed { code, message } => (
                LedgerOutcome::Failed,
                Some(code.map_or_else(|| message.clone(), |c| format!("({c}) {message}"))),
            ),
            StatementOutcome::NotRun => return Ok(()),
        };
        self.ledger.record(LedgerEntry {
            sequence: 0,
            run_id: ctx.run_id.clone(),
            recorded_at: Utc::now(),
            source: plan.source.label().to_owned(),
            job: planned.group.info.job.to_string(),
            rel_path: format!(
                "{}/{}",
                planned.group.info.artifact, planned.group.info.rel_path
            ),
            object_key: statement.object_key().map(|k| k.to_string()),
            statement_kind: statement.kind.label(),
            raw_fp: Some(statement.raw_fp),
            effective_fp: statement.effective_fp,
            outcome,
            detail,
            git_sha: ctx.git_sha.clone(),
        })?;
        Ok(())
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
