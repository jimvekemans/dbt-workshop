use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::executor::{FailurePolicy, HaltScope, PlanRunner, RunContext};
use crate::ledger::LedgerReader;
use crate::planner::Planner;
use crate::sources::{
    DeployedSnapshot, DiscoveredSource, SnapshotEntry, SourceDiscovery, SourceHandler,
    SourceLifecycle, SourceOutcome, open_reader,
};
use chrono::Utc;
use vdp_vaultspeed_domain::{
    ArtifactKind, Decision, Environment, GroupReport, JobName, RelPath, RunMode, RunReport,
    SkipReason, SourcePlan, SourceReport, StatementOutcome,
};

use crate::engine::milestone::MilestoneGuard;
use crate::engine::prepared::{ExecutedSource, Preparation, PreparedSource};
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
    /// The `deployed/` mirror of executed SQL; the engine ignores it in preview.
    pub snapshot: Option<DeployedSnapshot>,
    pub dags: Option<DagStage>,
}

/// Runs everything that is staged, in three explicit phases: [`discover_sources`](Self::discover_sources),
/// then for each source [`prepare_source`](Self::prepare_source) (load and plan) and
/// [`execute_source`](Self::execute_source). [`run`](Self::run) composes them.
#[derive(Debug)]
pub struct Engine {
    parts: EngineParts,
    /// The `deployed/` mirror. Only an applying engine has one: a preview never writes it.
    mirror: Option<DeployedSnapshot>,
}

impl Engine {
    /// Creates the engine; a preview engine drops the `deployed/` mirror, so it never writes it.
    pub fn new(mut parts: EngineParts) -> Self {
        let mirror = if parts.mode.is_preview() {
            None
        } else {
            parts.snapshot.take()
        };
        Self { parts, mirror }
    }

    /// Runs every discovered source in priority order. Each source is prepared only after the previous
    /// one was executed: planning reads the ledger, and the earlier sources' ledger entries must be in it.
    ///
    /// # Errors
    ///
    /// [`EngineError::Discovery`] when sources cannot be discovered; [`EngineError::Plan`] when a source cannot be planned
    /// (ledger read, catalog probe, no policy decided a group); [`EngineError::Execution`] when the ledger cannot be written.
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

        if p.mode.is_preview() {
            vdp_core::milestone::preview(
                "vaultspeed",
                format!("preview run {} on {}", p.context.run_id, p.environment),
            );
        } else {
            vdp_core::milestone::initialized(
                "vaultspeed",
                format!("apply run {} on {}", p.context.run_id, p.environment),
            );
        }

        for source in &self.discover_sources()? {
            let prepared = self.prepare_source(source)?;
            let executed = self.execute_source(prepared)?;
            report.sources.push(executed.report);
            if executed.halts_run {
                tracing::error!(source = %source.kind, "run halted by failure policy");
                break;
            }
        }
        report.finished_at = Some(Utc::now());
        Ok(report)
    }

    /// Everything the discoveries found, in the order the sources must run.
    ///
    /// # Errors
    ///
    /// [`EngineError::Discovery`] when a discovery fails (for example an unreadable staging directory).
    pub fn discover_sources(&self) -> Result<Vec<DiscoveredSource>, EngineError> {
        let mut guard = MilestoneGuard::new(
            "vaultspeed",
            format!("run {} failed", self.parts.context.run_id),
        );
        let mut sources = Vec::new();
        for discovery in &self.parts.discoveries {
            sources.extend(discovery.discover()?);
        }
        guard.disarm();
        sources.sort_by_key(|a| a.priority);
        tracing::info!(count = sources.len(), "sources discovered");
        Ok(sources)
    }

    /// Loads and plans one source. Reads the ledger (and the catalog, when probing) and changes nothing.
    ///
    /// # Errors
    ///
    /// [`EngineError::Plan`] when the source cannot be planned (ledger read, catalog probe, no policy decided a group). A
    /// source that cannot be read or loaded is not an error: it is marked unloadable and reported.
    pub fn prepare_source(&self, source: &DiscoveredSource) -> Result<PreparedSource, EngineError> {
        let p = &self.parts;
        let policy = p.failure.for_source(&source.kind);
        let guard = MilestoneGuard::new(
            source.kind.to_string(),
            format!("source {} failed", source.kind),
        );
        tracing::info!(source = %source.kind, "processing source");

        let content = open_reader(&source.location).read().and_then(|files| {
            let handler = p
                .handlers
                .iter()
                .find(|h| h.supports(&source.kind))
                .ok_or_else(|| crate::sources::SourceError::Unsupported(source.kind.to_string()))?;
            handler.load(source, &files)
        });
        let preparation = match content {
            Ok(content) => Preparation::Planned {
                plan: p
                    .planner
                    .plan(&source.kind, content.groups, p.ledger.as_ref())?,
                dags: content.dags,
            },
            Err(error) => Preparation::Unloadable {
                message: vdp_core::display_chain(&error),
            },
        };
        Ok(PreparedSource {
            source: source.clone(),
            policy,
            preparation,
            guard,
        })
    }

    /// Executes a prepared source and does everything that follows from it: the `deployed/` mirror, the
    /// DAGs and the housekeeping of the staged files. Consumes the plan, so it cannot run twice.
    ///
    /// # Errors
    ///
    /// [`EngineError::Execution`] when the ledger cannot be written (the run stops rather than leave changes unrecorded).
    /// Statement failures are not errors: they are in the report.
    pub fn execute_source(&self, prepared: PreparedSource) -> Result<ExecutedSource, EngineError> {
        let p = &self.parts;
        let PreparedSource {
            source,
            policy,
            preparation,
            mut guard,
        } = prepared;
        let mut report = SourceReport::new(source.kind.label());

        let (plan, dags) = match preparation {
            Preparation::Planned { plan, dags } => (plan, dags),
            Preparation::Unloadable { message } => {
                report.errors.push(message.clone());
                let halts_run = policy == FailurePolicy::HaltRun;
                if halts_run {
                    report.halted = Some(format!("run halted: {message}"));
                }
                self.housekeep(&source, &mut report);
                return Ok(ExecutedSource { report, halts_run });
            }
        };

        let run = p.runner.run(&plan, &p.context, policy)?;
        report.groups = run.groups;
        let skipped = report
            .groups
            .iter()
            .filter(|g| matches!(g.decision, Decision::Skip(_)))
            .count();
        if skipped > 0 {
            vdp_core::milestone::skipped(
                &source.kind.to_string(),
                format!("{skipped} group(s) skipped"),
            );
        }
        let mut halts_run = false;
        if let Some(halt) = run.halt {
            halts_run = halt.scope == HaltScope::Run;
            report.halted = Some(halt.to_string());
        }

        self.write_snapshot(&plan, &report.groups, &mut report.errors);

        if let Some(stage) = p.dags.as_ref().filter(|s| s.handles(&source.kind))
            && !dags.is_empty()
            && report.halted.is_none()
        {
            let (dags, error) = stage.process(&dags, p.mode);
            report.dags = dags;
            report.errors.extend(error);
        }

        self.housekeep(&source, &mut report);
        if !report.has_failures() && report.halted.is_none() {
            guard.complete(format!("source {} completed", source.kind));
        }
        Ok(ExecutedSource { report, halts_run })
    }

    /// Mirrors executed SQL to `deployed/`, one file per job, in execution order.
    fn write_snapshot(&self, plan: &SourcePlan, groups: &[GroupReport], errors: &mut Vec<String>) {
        let Some(snapshot) = &self.mirror else {
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
            if let Err(error) = snapshot.write(SnapshotEntry {
                artifact,
                rel_dir: &dir,
                job: &job,
                statements: &statements,
            }) {
                errors.push(format!(
                    "could not update deployed snapshot: {}",
                    vdp_core::display_chain(&error)
                ));
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
                Err(error) => report.errors.push(format!(
                    "housekeeping failed: {}",
                    vdp_core::display_chain(&error)
                )),
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

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::sync::Mutex;

    use tracing::Subscriber;
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context as LayerContext, Layer, SubscriberExt as _};
    use vdp_db::DbError;
    use vdp_vaultspeed_domain::RunId;
    use vdp_vaultspeed_sql::{ArtifactPipeline, NoRewrite, RoutineAwareSplitter};

    use super::*;
    use crate::config::VaultspeedSettings;
    use crate::executor::{NoClassification, PlanRunnerParts, StatementExecutor};
    use crate::ledger::MemoryLedger;
    use crate::sources::{ArtifactLoader, PerFileGrouping, PmlDiscovery, PmlHandler};
    use crate::wiring;

    #[derive(Debug)]
    struct Succeeds;

    impl StatementExecutor for Succeeds {
        fn execute(&self, _: &str) -> Result<Option<i64>, DbError> {
            Ok(None)
        }
    }

    #[derive(Debug)]
    struct Fails;

    impl StatementExecutor for Fails {
        fn execute(&self, _: &str) -> Result<Option<i64>, DbError> {
            Err(DbError {
                code: Some(1064),
                message: "syntax error".into(),
            })
        }
    }

    #[derive(Default)]
    struct MilestoneVisitor {
        milestone: Option<String>,
        subject: Option<String>,
    }

    impl Visit for MilestoneVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            match field.name() {
                "milestone" => self.milestone = Some(value.to_owned()),
                "subject" => self.subject = Some(value.to_owned()),
                _ => {}
            }
        }

        fn record_debug(&mut self, _: &Field, _: &dyn Debug) {}
    }

    type CapturedMilestones = Arc<Mutex<Vec<(tracing::Level, String, String)>>>;

    /// Baseline process-wide subscriber for tests so `NoSubscriber` never primes a static callsite
    /// as `Interest::never()` on a parallel thread before `with_default` runs.
    struct AlwaysInterested;

    impl Subscriber for AlwaysInterested {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn register_callsite(
            &self,
            _: &'static tracing::Metadata<'static>,
        ) -> tracing::subscriber::Interest {
            tracing::subscriber::Interest::always()
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, _: &tracing::Event<'_>) {}

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    fn ensure_test_tracing() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = tracing::subscriber::set_global_default(AlwaysInterested);
            tracing::callsite::rebuild_interest_cache();
        });
    }

    fn capture_milestones<R>(f: impl FnOnce() -> R) -> (R, Vec<(tracing::Level, String, String)>) {
        ensure_test_tracing();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(MilestoneLayer(Arc::clone(&captured)));
        let result = tracing::subscriber::with_default(subscriber, || {
            tracing::callsite::rebuild_interest_cache();
            f()
        });
        let events = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        (result, events)
    }

    struct MilestoneLayer(CapturedMilestones);

    impl<S: Subscriber> Layer<S> for MilestoneLayer {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: LayerContext<'_, S>) {
            let mut visitor = MilestoneVisitor::default();
            event.record(&mut visitor);
            if let Some(milestone) = visitor.milestone {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((
                        *event.metadata().level(),
                        milestone,
                        visitor.subject.unwrap_or_default(),
                    ));
            }
        }
    }

    fn pml_engine_parts(
        pml_dir: &std::path::Path,
        mode: RunMode,
        executor: Arc<dyn StatementExecutor>,
        ledger: Arc<MemoryLedger>,
        settings: &VaultspeedSettings,
    ) -> EngineParts {
        ensure_test_tracing();
        let pipeline =
            ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), vec![], Arc::new(NoRewrite))
                .with_default_database("edw");
        let loader = Arc::new(ArtifactLoader::new(
            ArtifactKind::Ddl,
            pipeline,
            Arc::new(PerFileGrouping),
        ));
        EngineParts {
            environment: Environment::LDV,
            mode,
            context: RunContext {
                run_id: RunId::from_string("test-run"),
                environment: "LDV".into(),
                git_sha: None,
            },
            discoveries: vec![Arc::new(PmlDiscovery::new(pml_dir.to_owned()))],
            handlers: vec![Arc::new(PmlHandler::new(loader))],
            lifecycles: vec![],
            planner: wiring::planner(settings, None),
            runner: PlanRunner::new(PlanRunnerParts {
                executor,
                classifier: Arc::new(NoClassification),
                ledger: ledger.clone(),
                skip_dependents: false,
            }),
            ledger,
            failure: wiring::failure_policies(settings),
            snapshot: None,
            dags: None,
        }
    }

    fn pml_engine(
        pml_dir: &std::path::Path,
        mode: RunMode,
        executor: Arc<dyn StatementExecutor>,
        ledger: Arc<MemoryLedger>,
        settings: &VaultspeedSettings,
    ) -> Engine {
        Engine::new(pml_engine_parts(pml_dir, mode, executor, ledger, settings))
    }

    #[test]
    fn source_run_emits_structured_milestone_events() {
        let dir = tempfile::tempdir().unwrap();
        let pml_dir = dir.path().join("pml");
        std::fs::create_dir_all(&pml_dir).unwrap();
        std::fs::write(pml_dir.join("01_init.sql"), "CREATE TABLE edw.a (id INT);").unwrap();

        let ledger = Arc::new(MemoryLedger::new());
        let settings = VaultspeedSettings::default();
        let ((), events) = capture_milestones(|| {
            let apply_report = pml_engine(
                &pml_dir,
                RunMode::Apply,
                Arc::new(Succeeds),
                Arc::clone(&ledger),
                &settings,
            )
            .run()
            .unwrap();
            assert_eq!(apply_report.sources.len(), 1);

            let preview_report = pml_engine(
                &pml_dir,
                RunMode::Preview,
                Arc::new(Succeeds),
                Arc::clone(&ledger),
                &settings,
            )
            .run()
            .unwrap();
            assert_eq!(preview_report.sources.len(), 1);
        });
        assert_eq!(
            events,
            vec![
                (
                    tracing::Level::INFO,
                    "initialized".into(),
                    "vaultspeed".into()
                ),
                (tracing::Level::INFO, "completed".into(), "PML".into()),
                (tracing::Level::INFO, "preview".into(), "vaultspeed".into()),
                (tracing::Level::INFO, "skipped".into(), "PML".into()),
                (tracing::Level::INFO, "completed".into(), "PML".into()),
            ]
        );
    }

    #[test]
    fn failing_source_emits_failed_milestone_without_completed() {
        let dir = tempfile::tempdir().unwrap();
        let pml_dir = dir.path().join("pml");
        std::fs::create_dir_all(&pml_dir).unwrap();
        std::fs::write(pml_dir.join("01_init.sql"), "CREATE TABLE edw.a (id INT);").unwrap();

        let ledger = Arc::new(MemoryLedger::new());
        let settings = VaultspeedSettings::default();
        let ((), events) = capture_milestones(|| {
            let report = pml_engine(
                &pml_dir,
                RunMode::Apply,
                Arc::new(Fails),
                Arc::clone(&ledger),
                &settings,
            )
            .run()
            .unwrap();
            assert_eq!(report.sources.len(), 1);
            assert!(report.sources[0].has_failures());
        });
        assert!(
            events
                .iter()
                .any(|e| *e == (tracing::Level::WARN, "failed".into(), "PML".into())),
            "expected WARN failed milestone for PML, got {events:?}"
        );
        assert!(
            events
                .iter()
                .all(|(_, milestone, _)| milestone != "completed"),
            "unexpected completed milestone after failure: {events:?}"
        );
    }

    #[test]
    fn planner_error_emits_failed_milestone_without_completed() {
        let dir = tempfile::tempdir().unwrap();
        let pml_dir = dir.path().join("pml");
        std::fs::create_dir_all(&pml_dir).unwrap();
        std::fs::write(pml_dir.join("01_init.sql"), "CREATE TABLE edw.a (id INT);").unwrap();

        let ledger = Arc::new(MemoryLedger::new());
        let settings = VaultspeedSettings::default();
        let mut parts = pml_engine_parts(
            &pml_dir,
            RunMode::Apply,
            Arc::new(Succeeds),
            ledger,
            &settings,
        );
        parts.planner = Planner::new(
            vec![],
            vec![],
            Arc::new(crate::planner::SequentialPhasing::new(vec![])),
        );

        let (res, events) = capture_milestones(|| Engine::new(parts).run());
        assert!(
            matches!(res, Err(EngineError::Plan(_))),
            "expected Plan error, got {res:?}"
        );

        assert_eq!(
            events.iter().filter(|(_, m, _)| m == "failed").count(),
            1,
            "expected exactly one failed milestone, got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| *e == (tracing::Level::WARN, "failed".into(), "PML".into())),
            "expected WARN failed milestone for PML, got {events:?}"
        );
        assert!(
            events
                .iter()
                .all(|(_, milestone, _)| milestone != "completed"),
            "unexpected completed milestone after planner error: {events:?}"
        );
    }

    #[test]
    fn discovery_error_emits_failed_milestone_without_completed() {
        #[derive(Debug)]
        struct FailingDiscovery;

        impl SourceDiscovery for FailingDiscovery {
            fn discover(&self) -> Result<Vec<DiscoveredSource>, crate::sources::SourceError> {
                Err(crate::sources::SourceError::Unsupported(
                    "broken discovery".into(),
                ))
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let ledger = Arc::new(MemoryLedger::new());
        let settings = VaultspeedSettings::default();
        let mut parts = pml_engine_parts(
            dir.path(),
            RunMode::Apply,
            Arc::new(Succeeds),
            ledger,
            &settings,
        );
        parts.discoveries = vec![Arc::new(FailingDiscovery)];

        let (res, events) = capture_milestones(|| Engine::new(parts).run());
        assert!(
            matches!(res, Err(EngineError::Discovery(_))),
            "expected Discovery error, got {res:?}"
        );

        assert_eq!(
            events.iter().filter(|(_, m, _)| m == "failed").count(),
            1,
            "expected exactly one failed milestone, got {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| *e == (tracing::Level::WARN, "failed".into(), "vaultspeed".into())),
            "expected WARN failed milestone for vaultspeed, got {events:?}"
        );
        assert!(
            events
                .iter()
                .all(|(_, milestone, _)| milestone != "completed"),
            "unexpected completed milestone after discovery error: {events:?}"
        );
    }

    #[test]
    fn source_error_and_discovery_render_path_and_cause_once() {
        #[derive(Debug)]
        struct FailingHandler;

        impl crate::sources::SourceHandler for FailingHandler {
            fn supports(&self, kind: &vdp_vaultspeed_domain::SourceKind) -> bool {
                *kind == vdp_vaultspeed_domain::SourceKind::Pml
            }

            fn load(
                &self,
                _source: &DiscoveredSource,
                _files: &[crate::sources::SourceFile],
            ) -> Result<crate::sources::SourceContent, crate::sources::SourceError> {
                Err(crate::sources::SourceError::Io {
                    path: std::path::PathBuf::from("/restricted/01_init.sql"),
                    source: std::io::Error::from_raw_os_error(13),
                })
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let pml_dir = dir.path().join("pml");
        std::fs::create_dir_all(&pml_dir).unwrap();
        std::fs::write(pml_dir.join("01_init.sql"), "SELECT 1;").unwrap();

        let ledger = Arc::new(MemoryLedger::new());
        let settings = VaultspeedSettings::default();
        let mut parts = pml_engine_parts(
            &pml_dir,
            RunMode::Apply,
            Arc::new(Succeeds),
            ledger,
            &settings,
        );
        parts.handlers = vec![Arc::new(FailingHandler)];

        let report = Engine::new(parts).run().unwrap();
        assert_eq!(
            report.sources[0].errors,
            vec!["/restricted/01_init.sql: Permission denied (os error 13)"]
        );
        assert_eq!(
            report.sources[0].halted.as_deref(),
            Some("run halted: /restricted/01_init.sql: Permission denied (os error 13)")
        );

        let discovery = EngineError::from(crate::sources::SourceError::Io {
            path: std::path::PathBuf::from("/restricted/01_init.sql"),
            source: std::io::Error::from_raw_os_error(13),
        });
        assert_eq!(
            vdp_core::display_chain(&discovery),
            "source discovery failed: /restricted/01_init.sql: Permission denied (os error 13)"
        );
    }
    /// Characterises how a whole run composes its sources: ordering, the sequential effect of one
    /// source's ledger writes on the next source's plan, preview, failure and halt handling, DAG
    /// publication and the milestone stream.
    mod run_semantics {
        use std::collections::BTreeMap;
        use std::path::{Path, PathBuf};

        use vdp_vaultspeed_dags::{
            DagAlterationPipeline, DagContext, DagError, DagNamer, DagPublisher, PublishBundle,
        };
        use vdp_vaultspeed_domain::{GenerationKind, SourceKind};

        use super::*;
        use crate::engine::DagStageParts;
        use crate::executor::DryRunExecutor;
        use crate::ledger::OverlayLedger;
        use crate::sources::{
            DeployedSnapshot, GenerationHandler, PmlLifecycle, SnapshotMode, SourceLocation,
        };

        /// Records every statement it is asked to run; statements containing a queued needle fail once.
        #[derive(Debug, Default)]
        struct Scripted {
            log: Mutex<Vec<String>>,
            fail_once: Mutex<Vec<&'static str>>,
        }

        impl Scripted {
            fn failing_once(needles: &[&'static str]) -> Arc<Self> {
                Arc::new(Self {
                    log: Mutex::default(),
                    fail_once: Mutex::new(needles.to_vec()),
                })
            }

            fn executed(&self) -> Vec<String> {
                self.log.lock().unwrap().clone()
            }
        }

        impl StatementExecutor for Scripted {
            fn execute(&self, sql: &str) -> Result<Option<i64>, DbError> {
                self.log.lock().unwrap().push(sql.to_owned());
                let mut fail = self.fail_once.lock().unwrap();
                if let Some(at) = fail.iter().position(|needle| sql.contains(needle)) {
                    fail.remove(at);
                    return Err(DbError {
                        code: Some(1064),
                        message: "syntax error".into(),
                    });
                }
                Ok(None)
            }
        }

        #[derive(Debug)]
        struct FixedDiscovery(Vec<DiscoveredSource>);

        impl SourceDiscovery for FixedDiscovery {
            fn discover(&self) -> Result<Vec<DiscoveredSource>, crate::sources::SourceError> {
                Ok(self.0.clone())
            }
        }

        #[derive(Debug)]
        struct KeepName;

        impl DagNamer for KeepName {
            fn target_name(&self, source_name: &str) -> String {
                source_name.to_owned()
            }
        }

        #[derive(Debug, Default)]
        struct RecordingPublisher(Mutex<Vec<PublishBundle>>);

        impl DagPublisher for RecordingPublisher {
            fn describe(&self) -> String {
                "recording".into()
            }

            fn publish(&self, bundle: &PublishBundle) -> Result<(), DagError> {
                self.0.lock().unwrap().push(bundle.clone());
                Ok(())
            }
        }

        struct Staging {
            dir: tempfile::TempDir,
        }

        impl Staging {
            fn new() -> Self {
                Self {
                    dir: tempfile::tempdir().unwrap(),
                }
            }

            fn root(&self) -> &Path {
                self.dir.path()
            }

            fn deployed(&self) -> PathBuf {
                self.root().join("deployed")
            }

            fn source(&self, label: &str, files: &[(&str, &str)]) -> DiscoveredSource {
                let kind = if label == "PML" {
                    SourceKind::Pml
                } else {
                    SourceKind::Generation(label.parse().unwrap())
                };
                let dir = self.root().join("staged").join(label);
                for (name, content) in files {
                    let path = dir.join(name);
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, content).unwrap();
                }
                DiscoveredSource {
                    priority: kind.default_priority(),
                    kind,
                    location: SourceLocation::Directory(dir),
                }
            }
        }

        fn ddl_loader() -> Arc<ArtifactLoader> {
            let pipeline =
                ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), vec![], Arc::new(NoRewrite))
                    .with_default_database("edw");
            Arc::new(ArtifactLoader::new(
                ArtifactKind::Ddl,
                pipeline,
                Arc::new(PerFileGrouping),
            ))
        }

        /// Sources are handed over by two discoveries, deliberately out of priority order.
        fn parts(
            staging: &Staging,
            sources: &[DiscoveredSource],
            mode: RunMode,
            executor: Arc<dyn StatementExecutor>,
            ledger: Arc<MemoryLedger>,
            settings: &VaultspeedSettings,
        ) -> EngineParts {
            let mut parts = pml_engine_parts(staging.root(), mode, executor, ledger, settings);
            let loader = ddl_loader();
            let (first, rest) = sources.split_at(sources.len().min(1));
            parts.discoveries = vec![
                Arc::new(FixedDiscovery(rest.to_vec())),
                Arc::new(FixedDiscovery(first.to_vec())),
            ];
            parts.handlers = vec![
                Arc::new(PmlHandler::new(loader.clone())),
                Arc::new(GenerationHandler::new(GenerationKind::Ddl).with_sql("", loader.clone())),
                Arc::new(
                    GenerationHandler::new(GenerationKind::Comp)
                        .with_sql("DDL", loader)
                        .with_dags(""),
                ),
                Arc::new(GenerationHandler::new(GenerationKind::Fmc).with_dags("")),
            ];
            parts.lifecycles = vec![Arc::new(PmlLifecycle::new(staging.deployed().join("PML")))];
            parts.snapshot = Some(DeployedSnapshot::new(
                staging.deployed(),
                BTreeMap::from([(ArtifactKind::Ddl, SnapshotMode::Overwrite)]),
            ));
            parts
        }

        fn with_dags(parts: &mut EngineParts, publisher: &Arc<RecordingPublisher>) {
            parts.dags = Some(DagStage::new(DagStageParts {
                pipeline: DagAlterationPipeline::new(vec![], vec![]),
                namer: Arc::new(KeepName),
                publisher: publisher.clone(),
                context: DagContext {
                    environment: Environment::LDV,
                    dataset_env: "rel1ldv".into(),
                    database: "edw".into(),
                    singlestore_host: "localhost".into(),
                    singlestore_port: 3306,
                },
                publish_kinds: vec![GenerationKind::Fmc, GenerationKind::Comp],
            }));
        }

        fn run(parts: EngineParts) -> (RunReport, Vec<(tracing::Level, String, String)>) {
            let (result, events) = capture_milestones(|| Engine::new(parts).run());
            (result.unwrap(), events)
        }

        fn milestones(events: &[(tracing::Level, String, String)]) -> Vec<String> {
            events
                .iter()
                .map(|(level, milestone, subject)| format!("{level} {milestone} {subject}"))
                .collect()
        }

        /// One line per source and per group: what the report says happened, nothing volatile.
        fn digest(report: &RunReport, root: &Path) -> Vec<String> {
            let mut lines = Vec::new();
            for source in &report.sources {
                lines.push(format!(
                    "source {} errors={:?} halted={:?} housekeeping={:?} dags={:?}",
                    source.source,
                    source.errors,
                    source.halted,
                    source.housekeeping,
                    source
                        .dags
                        .iter()
                        .map(|d| (d.target_file.as_str(), d.published, d.error.as_deref()))
                        .collect::<Vec<_>>(),
                ));
                for group in &source.groups {
                    let outcomes: Vec<_> = group
                        .statements
                        .iter()
                        .map(|s| format!("{:?}", s.outcome))
                        .collect();
                    lines.push(format!(
                        "  {} [{}] {} -> {}",
                        group.rel_path,
                        group.phase,
                        group.decision,
                        outcomes.join(", ")
                    ));
                }
            }
            let root = root.to_string_lossy();
            lines
                .into_iter()
                .map(|line| line.replace(root.as_ref(), "<root>"))
                .collect()
        }

        fn ledger_digest(ledger: &dyn LedgerReader) -> Vec<String> {
            ledger
                .entries()
                .unwrap()
                .iter()
                .map(|e| {
                    format!(
                        "{} {} {} {:?} {:?}",
                        e.sequence, e.source, e.rel_path, e.outcome, e.object_key
                    )
                })
                .collect()
        }

        fn staged_files(dir: &Path) -> Vec<String> {
            let mut found: Vec<String> = walkdir::WalkDir::new(dir)
                .sort_by_file_name()
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(|e| {
                    e.path()
                        .strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            found.sort();
            found
        }

        const CREATE_A: &str = "CREATE TABLE edw.a (id INT);";
        const CREATE_B: &str = "CREATE TABLE edw.b (id INT);";
        const CREATE_C: &str = "CREATE TABLE edw.c (id INT);";

        #[test]
        fn apply_prepares_each_source_after_the_previous_one_executed() {
            let staging = Staging::new();
            let sources = vec![
                staging.source("10_DDL", &[("01_a.sql", CREATE_A), ("02_b.sql", CREATE_B)]),
                staging.source("PML", &[("01_init.sql", CREATE_A)]),
            ];
            let executor = Arc::new(Scripted::default());
            let ledger = Arc::new(MemoryLedger::new());
            let settings = VaultspeedSettings::default();
            let (report, events) = run(parts(
                &staging,
                &sources,
                RunMode::Apply,
                executor.clone(),
                ledger.clone(),
                &settings,
            ));

            assert_eq!(report.mode, RunMode::Apply);
            assert!(report.finished_at.is_some());
            let labels: Vec<_> = report.sources.iter().map(|s| s.source.as_str()).collect();
            assert_eq!(labels, ["PML", "10_DDL"], "PML outranks generations");
            // Source 2's first file is planned only after the PML script ran, so it sees it in the
            // ledger and does not run `edw.a` a second time.
            assert_eq!(
                executor.executed(),
                ["CREATE TABLE edw.a (id INT)", "CREATE TABLE edw.b (id INT)"]
            );
            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source PML errors=[] halted=None housekeeping=[\"moved PML script 01_init.sql to <root>/deployed/PML/01_init.sql\"] dags=[]",
                    "  01_init.sql [ddl:non-rdv] execute: new -> Applied",
                    "source 10_DDL errors=[] halted=None housekeeping=[] dags=[]",
                    "  01_a.sql [ddl:non-rdv] skip: already applied (ledger #1) -> NotRun",
                    "  02_b.sql [ddl:non-rdv] execute: new -> Applied",
                ]
            );
            assert_eq!(
                ledger_digest(ledger.as_ref()),
                [
                    "1 PML DDL/01_init.sql Applied Some(\"relation:edw.a\")",
                    "2 10_DDL DDL/02_b.sql Applied Some(\"relation:edw.b\")",
                ]
            );
            assert_eq!(
                staged_files(&staging.deployed()),
                ["DDL/B.sql", "DDL/INIT.sql", "PML/01_init.sql"]
            );
            assert!(
                staged_files(&staging.root().join("staged/PML")).is_empty(),
                "the completed PML script left staging"
            );
            assert_eq!(
                milestones(&events),
                [
                    "INFO initialized vaultspeed",
                    "INFO completed PML",
                    "INFO skipped 10_DDL",
                    "INFO completed 10_DDL",
                ]
            );
        }

        #[test]
        fn preview_plans_later_sources_against_the_overlay_and_changes_nothing() {
            let staging = Staging::new();
            let sources = vec![
                staging.source("PML", &[("01_init.sql", CREATE_A)]),
                staging.source("10_DDL", &[("01_a.sql", CREATE_A), ("02_b.sql", CREATE_B)]),
            ];
            let base = Arc::new(MemoryLedger::new());
            let overlay = Arc::new(OverlayLedger::new(base.clone()).unwrap());
            let settings = VaultspeedSettings::default();
            let mut parts = parts(
                &staging,
                &sources,
                RunMode::Preview,
                Arc::new(DryRunExecutor),
                base.clone(),
                &settings,
            );
            // Exactly what the composition root wires for a preview.
            parts.runner = PlanRunner::new(PlanRunnerParts {
                executor: Arc::new(DryRunExecutor),
                classifier: Arc::new(NoClassification),
                ledger: overlay.clone(),
                skip_dependents: false,
            });
            parts.ledger = overlay.clone();
            let (report, events) = run(parts);

            assert_eq!(report.mode, RunMode::Preview);
            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source PML errors=[] halted=None housekeeping=[\"would move PML script 01_init.sql to <root>/deployed/PML/01_init.sql\"] dags=[]",
                    "  01_init.sql [ddl:non-rdv] execute: new -> Applied",
                    "source 10_DDL errors=[] halted=None housekeeping=[] dags=[]",
                    "  01_a.sql [ddl:non-rdv] skip: already applied (ledger #1) -> NotRun",
                    "  02_b.sql [ddl:non-rdv] execute: new -> Applied",
                ]
            );
            assert!(
                base.entries().unwrap().is_empty(),
                "the base ledger is untouched"
            );
            assert_eq!(overlay.pending().unwrap().len(), 2);
            assert!(
                !staging.deployed().exists(),
                "no deployed snapshot and no moved script in preview"
            );
            assert_eq!(
                staged_files(&staging.root().join("staged/PML")),
                ["01_init.sql"]
            );
            assert_eq!(
                milestones(&events),
                [
                    "INFO preview vaultspeed",
                    "INFO completed PML",
                    "INFO skipped 10_DDL",
                    "INFO completed 10_DDL",
                ]
            );
        }

        #[test]
        fn a_failing_source_does_not_stop_the_next_one_and_its_failure_is_planned_around() {
            let staging = Staging::new();
            let sources = vec![
                staging.source("PML", &[("01_init.sql", CREATE_A)]),
                staging.source("10_DDL", &[("01_b.sql", CREATE_B)]),
                staging.source("20_DDL", &[("01_b.sql", CREATE_B), ("02_c.sql", CREATE_C)]),
            ];
            let executor = Scripted::failing_once(&["edw.b"]);
            let ledger = Arc::new(MemoryLedger::new());
            let settings = VaultspeedSettings::default();
            let (report, events) = run(parts(
                &staging,
                &sources,
                RunMode::Apply,
                executor.clone(),
                ledger.clone(),
                &settings,
            ));

            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source PML errors=[] halted=None housekeeping=[\"moved PML script 01_init.sql to <root>/deployed/PML/01_init.sql\"] dags=[]",
                    "  01_init.sql [ddl:non-rdv] execute: new -> Applied",
                    "source 10_DDL errors=[] halted=None housekeeping=[] dags=[]",
                    "  01_b.sql [ddl:non-rdv] execute: new -> Failed { code: Some(1064), message: \"syntax error\" }",
                    "source 20_DDL errors=[] halted=None housekeeping=[] dags=[]",
                    "  01_b.sql [ddl:non-rdv] execute: new -> Applied",
                    "  02_c.sql [ddl:non-rdv] execute: new -> Applied",
                ]
            );
            assert!(report.sources[1].has_failures());
            assert_eq!(
                executor.executed(),
                [
                    "CREATE TABLE edw.a (id INT)",
                    "CREATE TABLE edw.b (id INT)",
                    "CREATE TABLE edw.b (id INT)",
                    "CREATE TABLE edw.c (id INT)",
                ]
            );
            assert_eq!(
                ledger_digest(ledger.as_ref()),
                [
                    "1 PML DDL/01_init.sql Applied Some(\"relation:edw.a\")",
                    "2 10_DDL DDL/01_b.sql Failed Some(\"relation:edw.b\")",
                    "3 20_DDL DDL/01_b.sql Applied Some(\"relation:edw.b\")",
                    "4 20_DDL DDL/02_c.sql Applied Some(\"relation:edw.c\")",
                ]
            );
            assert_eq!(
                milestones(&events),
                [
                    "INFO initialized vaultspeed",
                    "INFO completed PML",
                    "WARN failed 10_DDL",
                    "INFO completed 20_DDL",
                ]
            );
        }

        #[test]
        fn halt_source_skips_the_rest_of_that_source_and_the_run_goes_on() {
            let staging = Staging::new();
            let sources = vec![
                staging.source("10_DDL", &[("01_b.sql", CREATE_B), ("02_c.sql", CREATE_C)]),
                staging.source("20_DDL", &[("01_a.sql", CREATE_A)]),
            ];
            let executor = Scripted::failing_once(&["edw.b"]);
            let mut settings = VaultspeedSettings::default();
            settings.failure.ddl = crate::config::FailurePolicySetting::HaltSource;
            let (report, events) = run(parts(
                &staging,
                &sources,
                RunMode::Apply,
                executor.clone(),
                Arc::new(MemoryLedger::new()),
                &settings,
            ));

            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source 10_DDL errors=[] halted=Some(\"source halted: B (01_b.sql) failed\") housekeeping=[] dags=[]",
                    "  01_b.sql [ddl:non-rdv] execute: new -> Failed { code: Some(1064), message: \"syntax error\" }",
                    "  02_c.sql [ddl:non-rdv] skip: run halted -> NotRun",
                    "source 20_DDL errors=[] halted=None housekeeping=[] dags=[]",
                    "  01_a.sql [ddl:non-rdv] execute: new -> Applied",
                ]
            );
            assert_eq!(
                executor.executed(),
                ["CREATE TABLE edw.b (id INT)", "CREATE TABLE edw.a (id INT)"]
            );
            assert_eq!(
                milestones(&events),
                [
                    "INFO initialized vaultspeed",
                    "INFO skipped 10_DDL",
                    "WARN failed 10_DDL",
                    "INFO completed 20_DDL",
                ]
            );
        }

        #[test]
        fn halt_run_stops_before_the_next_source_is_even_looked_at() {
            let staging = Staging::new();
            let sources = vec![
                staging.source("PML", &[("01_init.sql", CREATE_A)]),
                staging.source("10_DDL", &[("01_b.sql", CREATE_B)]),
                staging.source("20_DDL", &[("01_c.sql", CREATE_C)]),
            ];
            let executor = Scripted::failing_once(&["edw.b"]);
            let ledger = Arc::new(MemoryLedger::new());
            let mut settings = VaultspeedSettings::default();
            settings.failure.ddl = crate::config::FailurePolicySetting::HaltRun;
            let (report, events) = run(parts(
                &staging,
                &sources,
                RunMode::Apply,
                executor.clone(),
                ledger.clone(),
                &settings,
            ));

            assert!(report.finished_at.is_some());
            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source PML errors=[] halted=None housekeeping=[\"moved PML script 01_init.sql to <root>/deployed/PML/01_init.sql\"] dags=[]",
                    "  01_init.sql [ddl:non-rdv] execute: new -> Applied",
                    "source 10_DDL errors=[] halted=Some(\"run halted: B (01_b.sql) failed\") housekeeping=[] dags=[]",
                    "  01_b.sql [ddl:non-rdv] execute: new -> Failed { code: Some(1064), message: \"syntax error\" }",
                ]
            );
            assert_eq!(
                executor.executed(),
                ["CREATE TABLE edw.a (id INT)", "CREATE TABLE edw.b (id INT)"]
            );
            assert_eq!(ledger.entries().unwrap().len(), 2);
            assert_eq!(
                milestones(&events),
                [
                    "INFO initialized vaultspeed",
                    "INFO completed PML",
                    "WARN failed 10_DDL",
                ],
                "the unreached source emits nothing"
            );
        }

        #[test]
        fn a_halting_pml_failure_leaves_the_script_staged_and_the_generations_untouched() {
            let staging = Staging::new();
            let sources = vec![
                staging.source("PML", &[("01_init.sql", CREATE_A)]),
                staging.source("10_DDL", &[("01_b.sql", CREATE_B)]),
            ];
            let executor = Scripted::failing_once(&["edw.a"]);
            let settings = VaultspeedSettings::default();
            let (report, events) = run(parts(
                &staging,
                &sources,
                RunMode::Apply,
                executor.clone(),
                Arc::new(MemoryLedger::new()),
                &settings,
            ));

            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source PML errors=[] halted=Some(\"run halted: INIT (01_init.sql) failed\") housekeeping=[\"kept PML script 01_init.sql in staging: it did not complete\"] dags=[]",
                    "  01_init.sql [ddl:non-rdv] execute: new -> Failed { code: Some(1064), message: \"syntax error\" }",
                ]
            );
            assert_eq!(executor.executed(), ["CREATE TABLE edw.a (id INT)"]);
            assert_eq!(
                staged_files(&staging.root().join("staged/PML")),
                ["01_init.sql"]
            );
            assert_eq!(
                milestones(&events),
                ["INFO initialized vaultspeed", "WARN failed PML"]
            );
        }

        #[test]
        fn dags_are_published_in_apply_rendered_only_in_preview_and_skipped_after_a_halt() {
            let dag = ("flow.py", "x = 1\n");
            let build = |mode, executor: Arc<dyn StatementExecutor>| {
                let staging = Staging::new();
                let sources = vec![
                    staging.source("30_FMC", &[dag]),
                    staging.source("40_COMP", &[("DDL/01_b.sql", CREATE_B), dag]),
                ];
                let publisher = Arc::new(RecordingPublisher::default());
                let mut settings = VaultspeedSettings::default();
                settings.failure.ddl = crate::config::FailurePolicySetting::HaltSource;
                let mut parts = parts(
                    &staging,
                    &sources,
                    mode,
                    executor,
                    Arc::new(MemoryLedger::new()),
                    &settings,
                );
                with_dags(&mut parts, &publisher);
                (staging, publisher, run(parts))
            };

            let (staging, publisher, (report, events)) =
                build(RunMode::Apply, Arc::new(Scripted::default()));
            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source 30_FMC errors=[] halted=None housekeeping=[] dags=[(\"flow.py\", true, None)]",
                    "source 40_COMP errors=[] halted=None housekeeping=[] dags=[(\"flow.py\", true, None)]",
                    "  01_b.sql [ddl:non-rdv] execute: new -> Applied",
                ]
            );
            assert_eq!(publisher.0.lock().unwrap().len(), 2);
            assert_eq!(
                milestones(&events),
                [
                    "INFO initialized vaultspeed",
                    "INFO completed 30_FMC",
                    "INFO completed 40_COMP",
                ]
            );

            let (staging, publisher, (report, _)) =
                build(RunMode::Preview, Arc::new(DryRunExecutor));
            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source 30_FMC errors=[] halted=None housekeeping=[] dags=[(\"flow.py\", false, None)]",
                    "source 40_COMP errors=[] halted=None housekeeping=[] dags=[(\"flow.py\", false, None)]",
                    "  01_b.sql [ddl:non-rdv] execute: new -> Applied",
                ]
            );
            assert!(publisher.0.lock().unwrap().is_empty());

            let (staging, publisher, (report, events)) =
                build(RunMode::Apply, Scripted::failing_once(&["edw.b"]));
            assert_eq!(
                digest(&report, staging.root()),
                [
                    "source 30_FMC errors=[] halted=None housekeeping=[] dags=[(\"flow.py\", true, None)]",
                    "source 40_COMP errors=[] halted=Some(\"source halted: B (01_b.sql) failed\") housekeeping=[] dags=[]",
                    "  01_b.sql [ddl:non-rdv] execute: new -> Failed { code: Some(1064), message: \"syntax error\" }",
                ]
            );
            assert_eq!(
                publisher.0.lock().unwrap().len(),
                1,
                "no DAGs from a halted source"
            );
            assert_eq!(
                milestones(&events),
                [
                    "INFO initialized vaultspeed",
                    "INFO completed 30_FMC",
                    "WARN failed 40_COMP",
                ]
            );
        }

        // The phase functions on their own.

        fn staged_engine(
            staging: &Staging,
            sources: &[DiscoveredSource],
            executor: Arc<Scripted>,
            ledger: Arc<MemoryLedger>,
        ) -> Engine {
            Engine::new(parts(
                staging,
                sources,
                RunMode::Apply,
                executor,
                ledger,
                &VaultspeedSettings::default(),
            ))
        }

        #[test]
        fn discover_sources_orders_by_priority_across_discoveries() {
            let staging = Staging::new();
            let sources = vec![
                staging.source("20_DDL", &[("01_a.sql", CREATE_A)]),
                staging.source("PML", &[("01_init.sql", CREATE_A)]),
                staging.source("10_DDL", &[("01_b.sql", CREATE_B)]),
            ];
            let engine = staged_engine(
                &staging,
                &sources,
                Arc::default(),
                Arc::new(MemoryLedger::new()),
            );
            let labels: Vec<_> = engine
                .discover_sources()
                .unwrap()
                .iter()
                .map(|s| s.kind.label().to_owned())
                .collect();
            assert_eq!(labels, ["PML", "10_DDL", "20_DDL"]);
        }

        #[test]
        fn a_prepared_source_can_be_inspected_and_nothing_has_happened_yet() {
            let staging = Staging::new();
            let source = staging.source("PML", &[("01_init.sql", CREATE_A)]);
            let executor = Arc::new(Scripted::default());
            let ledger = Arc::new(MemoryLedger::new());
            let engine = staged_engine(
                &staging,
                std::slice::from_ref(&source),
                executor.clone(),
                ledger.clone(),
            );

            let (prepared, events) = capture_milestones(|| engine.prepare_source(&source).unwrap());
            assert_eq!(prepared.source(), &source);
            assert_eq!(prepared.policy(), FailurePolicy::HaltRun);
            assert!(prepared.load_error().is_none());
            let decisions: Vec<_> = prepared
                .plan()
                .unwrap()
                .groups()
                .map(|(_, planned)| planned.decision.to_string())
                .collect();
            assert_eq!(decisions, ["execute: new"]);
            assert!(executor.executed().is_empty(), "preparing runs nothing");
            assert!(
                ledger.entries().unwrap().is_empty(),
                "preparing writes nothing"
            );
            assert_eq!(
                staged_files(&staging.root().join("staged/PML")),
                ["01_init.sql"]
            );
            assert!(!staging.deployed().exists());
            assert!(events.is_empty(), "no milestone until the source ends");
        }

        #[test]
        fn dropping_a_prepared_source_unexecuted_ends_it_as_failed() {
            let staging = Staging::new();
            let source = staging.source("PML", &[("01_init.sql", CREATE_A)]);
            let engine = staged_engine(
                &staging,
                std::slice::from_ref(&source),
                Arc::default(),
                Arc::new(MemoryLedger::new()),
            );
            let ((), events) = capture_milestones(|| drop(engine.prepare_source(&source).unwrap()));
            assert_eq!(milestones(&events), ["WARN failed PML"]);
        }

        #[test]
        fn executing_a_prepared_source_runs_its_plan_and_reports_it() {
            let staging = Staging::new();
            let source = staging.source("PML", &[("01_init.sql", CREATE_A)]);
            let executor = Arc::new(Scripted::default());
            let ledger = Arc::new(MemoryLedger::new());
            let engine = staged_engine(
                &staging,
                std::slice::from_ref(&source),
                executor.clone(),
                ledger.clone(),
            );

            let prepared = engine.prepare_source(&source).unwrap();
            let (executed, events) =
                capture_milestones(|| engine.execute_source(prepared).unwrap());
            assert!(!executed.halts_run);
            assert_eq!(executed.report.source, "PML");
            assert_eq!(executor.executed(), ["CREATE TABLE edw.a (id INT)"]);
            assert_eq!(ledger.entries().unwrap().len(), 1);
            assert_eq!(milestones(&events), ["INFO completed PML"]);
        }

        #[test]
        fn sources_prepared_up_front_cannot_see_each_others_effects() {
            // Why `run` prepares a source only after the previous one executed.
            let staging = Staging::new();
            let sources = vec![
                staging.source("PML", &[("01_init.sql", CREATE_A)]),
                staging.source("10_DDL", &[("01_a.sql", CREATE_A)]),
            ];
            let engine = staged_engine(
                &staging,
                &sources,
                Arc::default(),
                Arc::new(MemoryLedger::new()),
            );
            let decisions = |prepared: &PreparedSource| -> Vec<String> {
                prepared
                    .plan()
                    .unwrap()
                    .groups()
                    .map(|(_, planned)| planned.decision.to_string())
                    .collect()
            };

            let first = engine.prepare_source(&sources[0]).unwrap();
            let early = engine.prepare_source(&sources[1]).unwrap();
            assert_eq!(decisions(&early), ["execute: new"]);
            drop((first, early));

            let first = engine.prepare_source(&sources[0]).unwrap();
            engine.execute_source(first).unwrap();
            let late = engine.prepare_source(&sources[1]).unwrap();
            assert_eq!(decisions(&late), ["skip: already applied (ledger #1)"]);
            drop(late);
        }

        #[test]
        fn an_unloadable_source_prepares_with_its_error_and_executes_as_a_halt() {
            #[derive(Debug)]
            struct Unreadable;

            impl SourceHandler for Unreadable {
                fn supports(&self, kind: &SourceKind) -> bool {
                    *kind == SourceKind::Pml
                }

                fn load(
                    &self,
                    _: &DiscoveredSource,
                    _: &[crate::sources::SourceFile],
                ) -> Result<crate::sources::SourceContent, crate::sources::SourceError>
                {
                    Err(crate::sources::SourceError::Unsupported("broken".into()))
                }
            }

            let staging = Staging::new();
            let source = staging.source("PML", &[("01_init.sql", CREATE_A)]);
            let mut parts = parts(
                &staging,
                std::slice::from_ref(&source),
                RunMode::Apply,
                Arc::new(Scripted::default()),
                Arc::new(MemoryLedger::new()),
                &VaultspeedSettings::default(),
            );
            parts.handlers = vec![Arc::new(Unreadable)];
            let engine = Engine::new(parts);

            let prepared = engine.prepare_source(&source).unwrap();
            let message = prepared.load_error().unwrap().to_owned();
            assert!(prepared.plan().is_none());
            let (executed, events) =
                capture_milestones(|| engine.execute_source(prepared).unwrap());
            assert!(executed.halts_run, "PML failures halt the run");
            assert_eq!(executed.report.errors, std::slice::from_ref(&message));
            assert_eq!(
                executed.report.halted,
                Some(format!("run halted: {message}"))
            );
            assert_eq!(milestones(&events), ["WARN failed PML"]);
        }

        #[test]
        fn a_planning_error_aborts_preparation_with_one_failed_milestone() {
            let staging = Staging::new();
            let source = staging.source("PML", &[("01_init.sql", CREATE_A)]);
            let mut parts = parts(
                &staging,
                std::slice::from_ref(&source),
                RunMode::Apply,
                Arc::new(Scripted::default()),
                Arc::new(MemoryLedger::new()),
                &VaultspeedSettings::default(),
            );
            parts.planner = Planner::new(
                vec![],
                vec![],
                Arc::new(crate::planner::SequentialPhasing::new(vec![])),
            );
            let engine = Engine::new(parts);
            let (result, events) = capture_milestones(|| engine.prepare_source(&source));
            assert!(matches!(result, Err(EngineError::Plan(_))));
            assert_eq!(milestones(&events), ["WARN failed PML"]);
        }

        #[test]
        fn a_preview_engine_has_no_deployed_mirror_even_when_one_is_configured() {
            let staging = Staging::new();
            let source = staging.source("10_DDL", &[("01_b.sql", CREATE_B)]);
            let ledger = Arc::new(MemoryLedger::new());
            let engine = Engine::new(parts(
                &staging,
                std::slice::from_ref(&source),
                RunMode::Preview,
                Arc::new(DryRunExecutor),
                ledger,
                &VaultspeedSettings::default(),
            ));
            let prepared = engine.prepare_source(&source).unwrap();
            engine.execute_source(prepared).unwrap();
            assert!(!staging.deployed().exists());
        }
    }
}
