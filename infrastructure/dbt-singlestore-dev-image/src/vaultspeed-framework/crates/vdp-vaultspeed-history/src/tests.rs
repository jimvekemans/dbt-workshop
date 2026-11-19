use chrono::{DateTime, Duration, TimeZone, Utc};
use vdp_vaultspeed_domain::{
    AlterOp, ChangeEvent, ChangeOutcome, ChangeRecorder, Fingerprint, ObjectRef, ObjectType, RunId,
    StatementKind,
};

use super::*;

const ENV: &str = "LDV";

struct Feed {
    history: DuckDbHistory,
    tick: i64,
}

impl Feed {
    fn new() -> Self {
        Self {
            history: DuckDbHistory::in_memory().unwrap(),
            tick: 0,
        }
    }

    fn send(
        &mut self,
        kind: StatementKind,
        target: Option<ObjectRef>,
        sql: &str,
        outcome: ChangeOutcome,
    ) {
        self.tick += 1;
        let run_time: DateTime<Utc> =
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::seconds(self.tick);
        let event = ChangeEvent {
            run_id: RunId::from_string("run-1"),
            environment: ENV.into(),
            source: "PML".into(),
            job: "job".into(),
            rel_path: "a/b.sql".into(),
            group_id: "g".into(),
            phase: "ddl".into(),
            statement_index: 0,
            kind,
            target,
            raw_sql: sql.into(),
            effective_sql: sql.into(),
            raw_fp: Fingerprint::of(sql),
            effective_fp: Fingerprint::of(sql),
            applied_rules: vec!["r1".into(), "r2".into()],
            decision: "execute: new".into(),
            outcome,
            rows_affected: None,
            error_code: None,
            error_message: None,
            ledger_sequence: None,
            run_time,
            duration_ms: Some(3),
            git_sha: None,
        };
        self.history.record(&event).unwrap();
    }

    fn ok(&mut self, kind: StatementKind, target: &ObjectRef, sql: &str) {
        self.send(kind, Some(target.clone()), sql, ChangeOutcome::Applied);
    }
}

fn table(name: &str) -> ObjectRef {
    ObjectRef::new(ObjectType::Table, Some("edw"), name)
}

fn create(object: ObjectType) -> StatementKind {
    StatementKind::Create {
        object,
        or_replace: false,
        if_not_exists: false,
    }
}

fn count(h: &DuckDbHistory, sql: &str) -> i64 {
    h.lock().unwrap().query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn lifespan_follows_renames() {
    let mut f = Feed::new();
    let x = table("x");
    let create_sql = "CREATE TABLE edw.x (a INT)";
    f.ok(create(ObjectType::Table), &x, create_sql);
    f.send(
        StatementKind::Insert,
        Some(x.clone()),
        "INSERT INTO edw.x SELECT 1",
        ChangeOutcome::Applied,
    );
    f.ok(
        StatementKind::Alter {
            op: AlterOp::AddColumn {
                column: "b".into(),
                data_type: "INT".into(),
            },
        },
        &x,
        "ALTER TABLE edw.x ADD COLUMN b INT",
    );
    f.ok(
        StatementKind::Alter {
            op: AlterOp::Rename { to: table("x_old") },
        },
        &x,
        "ALTER TABLE edw.x RENAME TO edw.x_old",
    );
    f.ok(create(ObjectType::Table), &x, create_sql);
    f.ok(
        StatementKind::Drop {
            object: ObjectType::Table,
            if_exists: false,
        },
        &table("x_old"),
        "DROP TABLE edw.x_old",
    );

    let objects = f.history.objects(ENV, None).unwrap();
    assert_eq!(objects.len(), 2);
    assert_eq!(objects[0].status, "dropped");
    assert_eq!(objects[0].first_identifier, "edw.x");
    assert_eq!(objects[0].identifier, "edw.x_old");
    assert_eq!(objects[1].status, "live");
    assert_eq!(objects[1].origin, "created");
    assert_eq!(objects[1].identifier, "edw.x");
    let names: Vec<String> = {
        let conn = f.history.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT identifier FROM object_names WHERE object_id = ? ORDER BY valid_from")
            .unwrap();
        stmt.query_map([objects[0].object_id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(names, ["edw.x", "edw.x_old"]);

    let life = f.history.lifespan(ENV, "edw.x_old").unwrap();
    assert_eq!(life.len(), 5);
    assert_eq!(life[3].operation, "ALTER");
    assert_eq!(life[3].renamed_to.as_deref(), Some("edw.x_old"));
    assert_eq!(life[4].operation, "DROP");
    assert_eq!(life[2].column_name.as_deref(), Some("b"));
    assert_eq!(
        f.history
            .objects(ENV, Some(ObjectStatus::Live))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.history.lifespans(ENV, "edw.x").unwrap().len(), 2);
    // The latest user of the name is the second object, with a single statement.
    assert_eq!(f.history.lifespan(ENV, "edw.x").unwrap().len(), 1);

    let second_create = &f.history.lifespan(ENV, "edw.x").unwrap()[0];
    assert_eq!(second_create.creation_time, life[0].run_time);
    assert!(second_create.run_time > life[0].run_time);
    assert_eq!(count(&f.history, "SELECT count(*) FROM rename_history"), 1);
    assert_eq!(count(&f.history, "SELECT count(*) FROM live_objects"), 1);
    assert_eq!(count(&f.history, "SELECT count(*) FROM object_lifespan"), 6);
    assert_eq!(
        count(
            &f.history,
            "SELECT len(applied_rules) FROM statements LIMIT 1"
        ),
        2
    );
}

#[test]
fn failed_create_starts_nothing_and_skipped_links_to_live_object() {
    let mut f = Feed::new();
    let t = table("t");
    f.send(
        create(ObjectType::Table),
        Some(t.clone()),
        "CREATE TABLE edw.t (a INT)",
        ChangeOutcome::Failed,
    );
    assert_eq!(f.history.objects(ENV, None).unwrap(), []);
    assert_eq!(
        count(
            &f.history,
            "SELECT count(*) FROM statements WHERE success IS FALSE"
        ),
        1
    );

    f.ok(create(ObjectType::Table), &t, "CREATE TABLE edw.t (a INT)");
    f.send(
        StatementKind::Insert,
        Some(t),
        "INSERT INTO edw.t SELECT 1",
        ChangeOutcome::Skipped,
    );
    let life = f.history.lifespan(ENV, "edw.t").unwrap();
    assert_eq!(life.len(), 2);
    assert_eq!(life[1].success, None);
    assert_eq!(life[1].outcome, "skipped");
    assert_eq!(life[0].success, Some(true));
    assert_eq!(f.history.objects(ENV, None).unwrap().len(), 1);
}

#[test]
fn untracked_origin_when_first_seen_is_not_a_create() {
    let mut f = Feed::new();
    f.ok(
        StatementKind::Alter {
            op: AlterOp::Comment,
        },
        &table("old"),
        "ALTER TABLE edw.old COMMENT 'x'",
    );
    let objects = f.history.objects(ENV, None).unwrap();
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].origin, "untracked");
    // Dropping an unknown object is linked to an immediately ended untracked object.
    f.ok(
        StatementKind::Drop {
            object: ObjectType::Table,
            if_exists: false,
        },
        &table("ghost"),
        "DROP TABLE edw.ghost",
    );
    let ghost = f.history.lifespan(ENV, "edw.ghost").unwrap();
    assert_eq!(ghost.len(), 1);
    assert_eq!(
        f.history
            .objects(ENV, Some(ObjectStatus::Dropped))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn create_or_replace_view_continues_one_object() {
    let mut f = Feed::new();
    let view = ObjectRef::new(ObjectType::View, Some("edw"), "v");
    let kind = StatementKind::Create {
        object: ObjectType::View,
        or_replace: true,
        if_not_exists: false,
    };
    f.ok(
        kind.clone(),
        &view,
        "CREATE OR REPLACE VIEW edw.v AS SELECT 1",
    );
    f.ok(kind, &view, "CREATE OR REPLACE VIEW edw.v AS SELECT 2");
    assert_eq!(f.history.objects(ENV, None).unwrap().len(), 1);
    assert_eq!(f.history.lifespan(ENV, "edw.v").unwrap().len(), 2);
}

#[test]
fn reopening_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/history.duckdb");
    {
        let mut f = Feed {
            history: DuckDbHistory::open(&path).unwrap(),
            tick: 0,
        };
        f.ok(
            create(ObjectType::Table),
            &table("x"),
            "CREATE TABLE edw.x (a INT)",
        );
    }
    let history = DuckDbHistory::open(&path).unwrap();
    assert_eq!(history.objects(ENV, None).unwrap().len(), 1);
    assert_eq!(history.lifespan(ENV, "edw.x").unwrap().len(), 1);
}

#[test]
fn formatted_sql_is_multi_line() {
    let mut f = Feed::new();
    f.send(
        StatementKind::Select,
        None,
        "select a, b, c from edw.t join edw.u on t.id = u.id where a > 1 and b < 2 order by c",
        ChangeOutcome::Applied,
    );
    let formatted: String = f
        .history
        .lock()
        .unwrap()
        .query_row("SELECT formatted_sql FROM statements", [], |r| r.get(0))
        .unwrap();
    assert!(formatted.lines().count() > 3, "{formatted}");
    assert!(formatted.contains("SELECT"));
}

#[test]
fn a_satisfied_create_continues_the_live_object() {
    let mut f = Feed::new();
    let x = table("x");
    let sql = "CREATE TABLE edw.x (a INT)";
    f.ok(create(ObjectType::Table), &x, sql);
    // Re-run of the deployed group: "table already exists" is evidence, not a new table.
    f.send(
        create(ObjectType::Table),
        Some(x.clone()),
        sql,
        ChangeOutcome::Satisfied,
    );
    assert_eq!(count(&f.history, "SELECT count(*) FROM objects"), 1);
    assert_eq!(count(&f.history, "SELECT count(*) FROM live_objects"), 1);
}

#[test]
fn a_satisfied_create_of_an_unknown_table_is_untracked() {
    let mut f = Feed::new();
    let x = table("x");
    f.send(
        create(ObjectType::Table),
        Some(x),
        "CREATE TABLE edw.x (a INT)",
        ChangeOutcome::Satisfied,
    );
    assert_eq!(
        count(
            &f.history,
            "SELECT count(*) FROM objects WHERE origin = 'untracked' AND status = 'live'"
        ),
        1
    );
}

#[test]
fn a_satisfied_drop_of_nothing_invents_no_object() {
    let mut f = Feed::new();
    f.send(
        StatementKind::Drop {
            object: ObjectType::Table,
            if_exists: true,
        },
        Some(table("gone")),
        "DROP TABLE IF EXISTS edw.gone",
        ChangeOutcome::Satisfied,
    );
    assert_eq!(count(&f.history, "SELECT count(*) FROM objects"), 0);
    assert_eq!(
        count(
            &f.history,
            "SELECT count(*) FROM statements WHERE object_id IS NULL"
        ),
        1
    );
}
