//! The `vdp-vaultspeed` binary as a process: `vdp`'s commands and the VaultSpeed ones, and the object history.

use std::path::Path;
use std::process::{Command, Output};

use chrono::{Duration, TimeZone, Utc};
use vdp_vaultspeed_domain::{
    AlterOp, ChangeEvent, ChangeOutcome, ChangeRecorder, Fingerprint, ObjectRef, ObjectType, RunId,
    StatementKind,
};
use vdp_vaultspeed_history::DuckDbHistory;

const BIN: &str = env!("CARGO_BIN_EXE_vdp-vaultspeed");

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env_remove("DBT_DEPLOY_TARGET")
        .env_remove("TARGET_BRANCH")
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_VAULTSPEED_DIR")
        .env("VDP_NO_VAULT", "1")
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn event(tick: i64, kind: StatementKind, target: &ObjectRef, sql: &str) -> ChangeEvent {
    ChangeEvent {
        run_id: RunId::from_string("run-1"),
        environment: "LDV".into(),
        source: "PML".into(),
        job: "job".into(),
        rel_path: "a/b.sql".into(),
        group_id: "g".into(),
        phase: "ddl".into(),
        statement_index: 0,
        kind,
        target: Some(target.clone()),
        raw_sql: sql.into(),
        effective_sql: sql.into(),
        raw_fp: Fingerprint::of(sql),
        effective_fp: Fingerprint::of(sql),
        applied_rules: Vec::new(),
        decision: "execute: new".into(),
        outcome: ChangeOutcome::Applied,
        rows_affected: Some(0),
        error_code: None,
        error_message: None,
        ledger_sequence: None,
        run_time: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::seconds(tick),
        duration_ms: Some(12),
        git_sha: None,
    }
}

/// `edw.hub_x` created, then renamed to `edw.hub_x_old`.
fn seed(path: &Path) {
    let history = DuckDbHistory::open(path).unwrap();
    let table = |name| ObjectRef::new(ObjectType::Table, Some("edw"), name);
    let create = StatementKind::Create {
        object: ObjectType::Table,
        or_replace: false,
        if_not_exists: false,
    };
    let rename = StatementKind::Alter {
        op: AlterOp::Rename {
            to: table("hub_x_old"),
        },
    };
    history
        .record(&event(
            1,
            create,
            &table("hub_x"),
            "CREATE TABLE edw.hub_x (a INT)",
        ))
        .unwrap();
    history
        .record(&event(
            2,
            rename,
            &table("hub_x"),
            "ALTER TABLE edw.hub_x RENAME TO edw.hub_x_old",
        ))
        .unwrap();
}

#[test]
fn help_lists_the_core_and_the_vaultspeed_commands() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let help = stdout(&out);
    for command in [
        "vaultspeed",
        "dbt",
        "sources",
        "release",
        "jira",
        "sql",
        "qa",
        "env",
    ] {
        assert!(
            help.lines().any(|l| l.trim_start().starts_with(command)),
            "`{command}` is missing from:\n{help}"
        );
    }
    let out = run(dir.path(), &["vaultspeed", "--help"]);
    let help = stdout(&out);
    for command in ["deploy", "plan", "ledger", "rules", "dags", "history"] {
        assert!(
            help.lines().any(|l| l.trim_start().starts_with(command)),
            "`{command}` is missing from:\n{help}"
        );
    }
}

#[test]
fn release_serve_and_the_release_commands_of_vdp_are_commands_of_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["release", "serve", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    for option in ["--repo-dir", "--port", "--ui-dir", "--validate"] {
        assert!(stdout(&out).contains(option), "{option}: {}", stdout(&out));
    }
    let out = run(dir.path(), &["release", "analyse", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("--fixversion"), "{}", stdout(&out));
}

#[test]
fn an_unknown_command_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("unrecognized subcommand 'frobnicate'"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn history_objects_prints_the_seeded_object_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("history.duckdb");
    seed(&file);
    let path = file.to_str().unwrap();
    let out = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "objects",
            "--json",
            "--path",
            path,
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let objects: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let objects = objects.as_array().unwrap();
    assert_eq!(objects.len(), 1, "{objects:?}");
    assert_eq!(objects[0]["identifier"], "edw.hub_x_old");
    assert_eq!(objects[0]["first_identifier"], "edw.hub_x");
    assert_eq!(objects[0]["status"], "live");

    let dropped = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "objects",
            "--status",
            "dropped",
            "--json",
            "--path",
            path,
        ],
    );
    assert!(dropped.status.success(), "{}", stderr(&dropped));
    assert_eq!(stdout(&dropped).trim(), "[]");
}

#[test]
fn history_object_follows_the_rename() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("history.duckdb");
    seed(&file);
    let path = file.to_str().unwrap();
    // Either name finds the object; the statements are its whole life, oldest first.
    for name in ["edw.hub_x", "edw.hub_x_old"] {
        let out = run(
            dir.path(),
            &[
                "--env",
                "LDV",
                "vaultspeed",
                "history",
                "--path",
                path,
                "object",
                name,
            ],
        );
        assert!(out.status.success(), "{}", stderr(&out));
        let table = stdout(&out);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 3, "{table}");
        assert!(lines[0].starts_with("run_time"), "{table}");
        assert!(
            lines[1].contains("edw.hub_x") && lines[1].contains("applied"),
            "{table}"
        );
        assert!(lines[2].contains("12 ms"), "{table}");
    }
    let json = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "object",
            "edw.hub_x",
            "--json",
            "--path",
            path,
        ],
    );
    let statements: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(statements.as_array().unwrap().len(), 2);
    assert_eq!(statements[1]["renamed_to"], "edw.hub_x_old");

    let all = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "object",
            "edw.hub_x",
            "--all",
            "--json",
            "--path",
            path,
        ],
    );
    let lifespans: serde_json::Value = serde_json::from_str(&stdout(&all)).unwrap();
    assert_eq!(lifespans.as_array().unwrap().len(), 1);

    let missing = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "object",
            "edw.nope",
            "--path",
            path,
        ],
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("edw.nope"));
}

#[test]
fn history_without_a_file_says_so_and_creates_none() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("none.duckdb");
    let out = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "objects",
            "--path",
            file.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("no object history"),
        "{}",
        stderr(&out)
    );
    assert!(!file.exists());
}

#[test]
fn history_tables_match_golden_output() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("history.duckdb");
    seed(&file);
    let path = file.to_str().unwrap();

    let single_out = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "--path",
            path,
            "object",
            "edw.hub_x",
        ],
    );
    assert!(single_out.status.success(), "{}", stderr(&single_out));
    assert_eq!(
        stdout(&single_out),
        concat!(
            "run_time              operation                 identifier  outcome  rows  duration\n",
            "2026-01-01T00:00:01Z  CREATE/CREATE TABLE       edw.hub_x   applied  0     12 ms\n",
            "2026-01-01T00:00:02Z  ALTER/ALTER TABLE RENAME  edw.hub_x   applied  0     12 ms\n",
        )
    );

    let list_out = run(
        dir.path(),
        &[
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "--path",
            path,
            "objects",
        ],
    );
    assert!(list_out.status.success(), "{}", stderr(&list_out));
    assert_eq!(
        stdout(&list_out),
        concat!(
            "id  type   identifier     first_identifier  status  origin   created               ended\n",
            "1   table  edw.hub_x_old  edw.hub_x         live    created  2026-01-01T00:00:01Z  -\n",
        )
    );
}

#[test]
fn missing_history_object_warning_is_captured_in_ndjson_log() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("history.duckdb");
    seed(&file);
    let log = dir.path().join("missing.json");

    let out = run(
        dir.path(),
        &[
            "--log-json",
            log.to_str().unwrap(),
            "--env",
            "LDV",
            "vaultspeed",
            "history",
            "object",
            "edw.nope",
            "--path",
            file.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("no object named edw.nope in the LDV history"),
        "{}",
        stderr(&out)
    );

    let content = std::fs::read_to_string(&log).unwrap();
    let events: Vec<serde_json::Value> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        events.iter().any(|record| {
            record["level"] == "WARN"
                && record["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("no object named edw.nope in the LDV history"))
        }),
        "expected WARN event in NDJSON log, got: {content}"
    );
}

#[test]
fn dags_render_missing_file_prints_path_and_cause_once() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.py");
    let os_cause = std::fs::read_to_string(&missing).unwrap_err().to_string();

    let out = run(
        dir.path(),
        &["--no-vault", "vaultspeed", "dags", "render", "missing.py"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        stderr(&out),
        format!("vdp-vaultspeed: error: {}: {os_cause}\n", missing.display())
    );
}
