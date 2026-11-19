use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::executor::{HaltScope, PlanRunner, RunContext};
use crate::ledger::LedgerReader;
use crate::planner::Planner;
use crate::sources::{
    DeployedSnapshot, DiscoveredSource, SourceDiscovery, SourceHandler, SourceLifecycle,
    SourceOutcome, open_reader,
};
use chrono::Utc;
use vdp_vaultspeed_domain::{
    ArtifactKind, Decision, Environment, GroupReport, JobName, RelPath, RunMode, RunReport,
    SkipReason, SourcePlan, SourceReport, StatementOutcome,
};

use crate::engine::{DagStage, EngineError, FailurePolicies};

/// Everything the engine is composed of.
#[derive(Debug)]
pub struct EngineParts {
    pub environment: Environment,
    pub mode: RunMode,
    pub context: RunContext,
    pub discoveries: Vec<Arc<dyn SourceDiscovery>>,
    pub handlers: Vec<Arc<dyn SourceHandler>>,
    pub lifecycles: Vec<Arc<dyn SourceLifecycle>>,
    pub planner: Planner,
    pub runner: PlanRunner,
    /// Must read the same ledger the runner writes to.
    pub ledger: Arc<dyn LedgerReader>,
    pub failure: FailurePolicies,
    pub snapshot: Option<DeployedSnapshot>,
    pub dags: Option<DagStage>,
}

/// Runs everything that is staged.
#[derive(Debug)]
pub struct Engine {
    parts: EngineParts,
}

impl Engine {
    pub const fn new(parts: EngineParts) -> Self {
        Self { parts }
    }

    pub fn run(&self) -> Result<RunReport, EngineError> {
        let p = &self.parts;
        let mut report = RunReport {
            run_id: p.context.run_id.clone(),
            environment: p.environment,
            mode: p.mode,
            started_at: Utc::now(),
            finished_at: None,
            sources: Vec::new(),
        };

        let mut sources = Vec::new();
        for discovery in &p.discoveries {
            sources.extend(discovery.discover()?);
        }
        sources.sort_by_key(|a| a.priority);
        tracing::info!(count = sources.len(), "sources discovered");

        for source in &sources {
            let (source_report, halt_run) = self.process(source)?;
            report.sources.push(source_report);
            if halt_run {
                tracing::error!(source = %source.kind, "run halted by failure policy");
                break;
            }
        }
        report.finished_at = Some(Utc::now());
        Ok(report)
    }

    /// Returns the report and whether the whole run must stop.
    fn process(&self, source: &DiscoveredSource) -> Result<(SourceReport, bool), EngineError> {
        let p = &self.parts;
        let mut report = SourceReport::new(source.kind.label());
        let policy = p.failure.for_source(&source.kind);
        tracing::info!(source = %source.kind, "processing source");

        let content = open_reader(&source.location).read().and_then(|files| {
            let handler = p
                .handlers
                .iter()
                .find(|h| h.supports(&source.kind))
                .ok_or_else(|| crate::sources::SourceError::Unsupported(source.kind.to_string()))?;
            handler.load(source, &files)
        });
        let content = match content {
            Ok(content) => content,
            Err(error) => {
                report.errors.push(error.to_string());
                let halt_run = policy == crate::executor::FailurePolicy::HaltRun;
                if halt_run {
                    report.halted = Some(format!("run halted: {error}"));
                }
                self.housekeep(source, &mut report);
                return Ok((report, halt_run));
            }
        };

        let plan = p
            .planner
            .plan(&source.kind, content.groups, p.ledger.as_ref())?;
        let run = p.runner.run(&plan, &p.context, policy)?;
        report.groups = run.groups;
        let mut halt_run = false;
        if let Some(halt) = run.halt {
            halt_run = halt.scope == HaltScope::Run;
            report.halted = Some(halt.to_string());
        }

        if !p.mode.is_preview() {
            self.write_snapshot(&plan, &report.groups, &mut report.errors);
        }

        if let Some(stage) = p.dags.as_ref().filter(|s| s.handles(&source.kind))
            && !content.dags.is_empty()
            && report.halted.is_none()
        {
            let (dags, error) = stage.process(&content.dags, p.mode);
            report.dags = dags;
            report.errors.extend(error);
        }

        self.housekeep(source, &mut report);
        Ok((report, halt_run))
    }

    /// Mirrors executed SQL to `deployed/`, one file per job, in execution order.
    fn write_snapshot(&self, plan: &SourcePlan, groups: &[GroupReport], errors: &mut Vec<String>) {
        let Some(snapshot) = &self.parts.snapshot else {
            return;
        };
        let mut files: BTreeMap<(ArtifactKind, RelPath, JobName), Vec<&str>> = BTreeMap::new();
        for ((_, planned), group) in plan.groups().zip(groups) {
            for statement in &group.statements {
                if matches!(
                    statement.outcome,
                    StatementOutcome::Applied | StatementOutcome::Satisfied { .. }
                ) {
                    let info = &planned.group.info;
                    files
                        .entry((info.artifact, info.rel_path.parent(), info.job.clone()))
                        .or_default()
                        .push(&statement.effective_sql);
                }
            }
        }
        for ((artifact, dir, job), statements) in files {
            if let Err(error) = snapshot.write(artifact, &dir, &job, &statements) {
                errors.push(format!("could not update deployed snapshot: {error}"));
            }
        }
    }

    fn housekeep(&self, source: &DiscoveredSource, report: &mut SourceReport) {
        let outcome = SourceOutcome {
            source,
            succeeded: !report.has_failures() && report.halted.is_none(),
            completed_files: completed_files(&report.groups),
        };
        for lifecycle in self
            .parts
            .lifecycles
            .iter()
            .filter(|l| l.supports(&source.kind))
        {
            match lifecycle.complete(&outcome, self.parts.mode) {
                Ok(notes) => report.housekeeping.extend(notes),
                Err(error) => report.errors.push(format!("housekeeping failed: {error}")),
            }
        }
    }
}

/// Files whose every group completed: executed without failure, or skipped because the
/// change is already in effect or deliberately excluded.
fn completed_files(groups: &[GroupReport]) -> BTreeSet<RelPath> {
    let mut incomplete = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for group in groups {
        let path = RelPath::new(&group.rel_path);
        let done = match &group.decision {
            Decision::Execute(_) => !group.failed(),
            Decision::Skip(
                SkipReason::AlreadyApplied { .. }
                | SkipReason::Satisfied { .. }
                | SkipReason::Excluded { .. }
                | SkipReason::DuplicateInPlan { .. },
            ) => true,
            Decision::Skip(SkipReason::Halted | SkipReason::UpstreamFailed { .. })
            | Decision::Refuse { .. } => false,
        };
        if !done {
            incomplete.insert(path.clone());
        }
        seen.insert(path);
    }
    seen.difference(&incomplete).cloned().collect()
}
