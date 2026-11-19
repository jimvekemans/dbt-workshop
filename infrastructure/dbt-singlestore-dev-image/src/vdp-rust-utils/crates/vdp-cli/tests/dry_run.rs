//! CLI `--dry-run` preview tests for `sql run`.
#![cfg(feature = "sql")]

use std::process::Command;

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

#[test]
fn sql_run_dry_run_prints_golden_preview_and_skips_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("check.sql");
    std::fs::write(&script, "SELECT 42;\n").unwrap();
    let csv_dir = dir.path().join("out_csv");

    let sql_out = Command::new(VDP)
        .args([
            "--no-vault",
            "sql",
            "run",
            script.to_str().unwrap(),
            "--csv-dir",
            csv_dir.to_str().unwrap(),
            "--dry-run",
        ])
        .current_dir(dir.path())
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_PROJECT_DIR")
        .env_remove("DBT_DEPLOY_TARGET")
        .output()
        .unwrap();
    assert!(
        sql_out.status.success(),
        "sql run --dry-run failed: {}",
        String::from_utf8_lossy(&sql_out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&sql_out.stdout),
        format!(
            "== {}\nwould run {} (11 bytes)\nwould save {}\n",
            script.display(),
            script.display(),
            csv_dir.join("check.csv").display()
        )
    );
    assert!(!csv_dir.exists(), "dry-run must not create csv_dir");
}

#[test]
fn sql_run_missing_file_prints_path_and_cause_once() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.sql");
    let os_cause = std::fs::read_to_string(&missing).unwrap_err().to_string();

    let out = Command::new(VDP)
        .args(["--no-vault", "sql", "run", "missing.sql", "--dry-run"])
        .current_dir(dir.path())
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_PROJECT_DIR")
        .env_remove("DBT_DEPLOY_TARGET")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!("vdp: error: missing.sql: {os_cause}\n")
    );
}
