//! Planner decisions end to end: SQL text is classified by the real pipeline, applied through the real
//! runner into a [`MemoryLedger`], and planned again, so the ledger entries are exactly what a run writes.

use std::sync::Arc;

use vdp_db::DbError;
use vdp_vaultspeed_domain::{
    ArtifactKind, Decision, ExecuteReason, JobInfo, JobName, Layer, ObjectRef, OrderKey, RelPath,
    RunId, SkipReason, SourceKind, Statement, StatementGroup,
};
use vdp_vaultspeed_sql::{ArtifactPipeline, NoRewrite, RoutineAwareSplitter};

use crate::config::{StandaloneDropsSetting, VaultspeedSettings};
use crate::executor::{
    FailurePolicy, NoClassification, PlanRunner, PlanRunnerParts, RunContext, StatementExecutor,
};
use crate::ledger::{LedgerReader, MemoryLedger};
use crate::planner::{CatalogProbe, Planner, ProbeError, ProbeVerdict};
use crate::sources::{DropCreatePairing, GroupingStrategy};
use crate::wiring;

#[derive(Debug)]
struct Succeeds;

impl StatementExecutor for Succeeds {
    fn execute(&self, _: &str) -> Result<Option<i64>, DbError> {
        Ok(None)
    }
}

#[derive(Debug)]
struct Catalog(Option<bool>);

impl CatalogProbe for Catalog {
    fn verify(&self, _: &Statement) -> Result<ProbeVerdict, ProbeError> {
        Ok(ProbeVerdict::Unknown)
    }

    fn exists(&self, _: &ObjectRef) -> Result<Option<bool>, ProbeError> {
        Ok(self.0)
    }
}

struct Lab {
    ledger: Arc<MemoryLedger>,
    pipeline: ArtifactPipeline,
    planner: Planner,
}

impl Lab {
    fn new(database: &str, probe: Option<Catalog>) -> Self {
        let mut settings = VaultspeedSettings::default();
        settings.planning.standalone_drops = StandaloneDropsSetting::Execute;
        let probe = probe.map(|catalog| Arc::new(catalog) as Arc<dyn CatalogProbe>);
        Self {
            ledger: Arc::new(MemoryLedger::new()),
            pipeline: ArtifactPipeline::new(
                Arc::new(RoutineAwareSplitter),
                vec![],
                Arc::new(NoRewrite),
            )
            .with_default_database(database),
            planner: wiring::planner(&settings, probe),
        }
    }

    fn groups(&self, sql: &str, order: u32) -> Vec<StatementGroup> {
        let statements = self.pipeline.statements(sql).unwrap();
        DropCreatePairing
            .group(statements)
            .into_iter()
            .enumerate()
            .map(|(ordinal, statements)| StatementGroup {
                id: vdp_vaultspeed_domain::GroupId::new("PML", &RelPath::new("t.sql"), ordinal),
                info: JobInfo {
                    artifact: ArtifactKind::Ddl,
                    job: JobName::from_file_stem("T"),
                    rel_path: RelPath::new("t.sql"),
                    layer: Layer::NotApplicable,
                    order: OrderKey::new(vec![order]),
                },
                ordinal,
                statements,
            })
            .collect()
    }

    fn plan(&self, sql: &str, order: u32) -> Vec<Decision> {
        let plan = self
            .planner
            .plan(
                &SourceKind::Pml,
                self.groups(sql, order),
                self.ledger.as_ref(),
            )
            .unwrap();
        plan.groups().map(|(_, p)| p.decision.clone()).collect()
    }

    /// Plans and runs the script; what the planner decided to execute lands in the ledger.
    fn deploy(&self, sql: &str) {
        let plan = self
            .planner
            .plan(&SourceKind::Pml, self.groups(sql, 1), self.ledger.as_ref())
            .unwrap();
        let runner = PlanRunner::new(PlanRunnerParts {
            executor: Arc::new(Succeeds),
            classifier: Arc::new(NoClassification),
            ledger: self.ledger.clone(),
            skip_dependents: false,
        });
        let ctx = RunContext {
            run_id: RunId::from_string("lab"),
            environment: "LDV".into(),
            git_sha: None,
        };
        runner.run(&plan, &ctx, FailurePolicy::Continue).unwrap();
    }

    fn only(&self, sql: &str) -> Decision {
        let mut decisions = self.plan(sql, 1);
        assert_eq!(decisions.len(), 1, "{decisions:?}");
        decisions.remove(0)
    }
}

const CREATE_X: &str = "CREATE TABLE s.x (a INT)";

#[test]
fn a_create_after_its_table_was_renamed_away_runs_again() {
    let lab = Lab::new("", None);
    lab.deploy(CREATE_X);
    lab.deploy("ALTER TABLE s.x RENAME TO s.x_old");
    assert_eq!(
        lab.only(CREATE_X),
        Decision::Execute(ExecuteReason::Stale { since: 2 })
    );
    // The renamed table itself is what lives under the new name.
    let history = lab.ledger.object_history("relation:s.x_old").unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].renamed_to.as_deref(), Some("relation:s.x_old"));
}

#[test]
fn a_rename_round_trip_leaves_the_create_applied() {
    let lab = Lab::new("", None);
    lab.deploy(CREATE_X);
    lab.deploy("ALTER TABLE s.x RENAME TO s.y");
    lab.deploy("ALTER TABLE s.y RENAME TO s.x");
    assert_eq!(
        lab.only(CREATE_X),
        Decision::Skip(SkipReason::AlreadyApplied { sequence: 1 })
    );
}

#[test]
fn a_create_after_a_drop_runs_again_and_standing_creates_skip() {
    let lab = Lab::new("", None);
    lab.deploy(CREATE_X);
    assert_eq!(
        lab.only(CREATE_X),
        Decision::Skip(SkipReason::AlreadyApplied { sequence: 1 })
    );
    lab.deploy("DROP TABLE s.x");
    assert_eq!(
        lab.only(CREATE_X),
        Decision::Execute(ExecuteReason::Stale { since: 2 })
    );
}

#[test]
fn a_drop_and_create_pair_that_stands_is_skipped_as_a_whole() {
    let lab = Lab::new("", None);
    let pair = "DROP TABLE IF EXISTS s.x; CREATE TABLE s.x (a INT)";
    lab.deploy(pair);
    lab.deploy("ALTER TABLE s.x RENAME TO s.y");
    lab.deploy("ALTER TABLE s.y RENAME TO s.x");
    assert_eq!(
        lab.only(pair),
        Decision::Skip(SkipReason::AlreadyApplied { sequence: 2 })
    );
}

#[test]
fn a_rebuild_of_a_table_that_came_from_a_rename_is_refused() {
    let lab = Lab::new("", None);
    lab.deploy("CREATE TABLE s.a (a INT)");
    lab.deploy("ALTER TABLE s.a RENAME TO s.y");
    // Order number 5 is protected: DROP + CREATE with another definition would destroy a's data.
    let decisions = lab.plan("DROP TABLE IF EXISTS s.y; CREATE TABLE s.y (b INT)", 5);
    let [Decision::Refuse { reason }] = decisions.as_slice() else {
        panic!("expected a refusal, got {decisions:?}");
    };
    assert!(reason.starts_with("rebuild-guard"), "{reason}");
}

#[test]
fn unqualified_pml_renames_use_the_environments_database() {
    let lab = Lab::new("edw", None);
    lab.deploy("CREATE TABLE edw.x (a INT)");
    lab.deploy("ALTER TABLE x RENAME TO x_old");
    assert_eq!(
        lab.only("CREATE TABLE edw.x (a INT)"),
        Decision::Execute(ExecuteReason::Stale { since: 2 })
    );
    // Without the default database the two spellings stay different objects (the old bug).
    let blind = Lab::new("", None);
    blind.deploy("CREATE TABLE edw.x (a INT)");
    blind.deploy("ALTER TABLE x RENAME TO x_old");
    assert!(matches!(
        blind.only("CREATE TABLE edw.x (a INT)"),
        Decision::Skip(SkipReason::AlreadyApplied { .. })
    ));
}

#[test]
fn a_probe_that_misses_the_table_makes_an_applied_create_run_again() {
    let lab = Lab::new("", Some(Catalog(Some(false))));
    lab.deploy(CREATE_X);
    let decision = lab.only(CREATE_X);
    let Decision::Execute(ExecuteReason::Vanished { detail }) = decision else {
        panic!("expected vanished, got {decision:?}");
    };
    assert!(detail.contains("s.x"), "{detail}");

    // Present, or unknown to the probe: the ledger stands.
    for answer in [Some(true), None] {
        let lab = Lab::new("", Some(Catalog(answer)));
        lab.deploy(CREATE_X);
        assert!(matches!(
            lab.only(CREATE_X),
            Decision::Skip(SkipReason::AlreadyApplied { .. })
        ));
    }
}

#[test]
fn rows_and_columns_travel_with_a_rename_round_trip() {
    let lab = Lab::new("", None);
    lab.deploy(CREATE_X);
    lab.deploy("INSERT INTO s.x VALUES (1)");
    lab.deploy("ALTER TABLE s.x ADD COLUMN b INT");
    lab.deploy("ALTER TABLE s.x RENAME TO s.y");
    lab.deploy("ALTER TABLE s.y RENAME TO s.x");
    // Same table, same rows: re-running the seed would duplicate them.
    assert!(matches!(
        lab.only("INSERT INTO s.x VALUES (1)"),
        Decision::Skip(SkipReason::AlreadyApplied { .. })
    ));
    assert!(matches!(
        lab.only("ALTER TABLE s.x ADD COLUMN b INT"),
        Decision::Skip(SkipReason::AlreadyApplied { .. })
    ));
}

#[test]
fn rows_of_a_dropped_and_recreated_table_are_stale() {
    let lab = Lab::new("", None);
    lab.deploy(CREATE_X);
    lab.deploy("INSERT INTO s.x VALUES (1)");
    lab.deploy("DROP TABLE s.x");
    lab.deploy("CREATE TABLE s.x (a INT, b INT)");
    assert!(matches!(
        lab.only("INSERT INTO s.x VALUES (1)"),
        Decision::Execute(ExecuteReason::Stale { .. })
    ));
}
