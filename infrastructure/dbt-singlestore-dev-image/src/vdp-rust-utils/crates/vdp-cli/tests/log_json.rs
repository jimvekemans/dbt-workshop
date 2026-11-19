//! Opt-in NDJSON run-log integration tests (`--log-json` and `VDP_LOG_JSON`).
#![cfg(feature = "release")]

use std::path::Path;
use std::process::{Command, Output};

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

fn vdp_cmd(dir: &Path, args: &[&str]) -> Output {
    Command::new(VDP)
        .args(args)
        .current_dir(dir)
        .env_remove("DBT_DEPLOY_TARGET")
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_LOG")
        .env_remove("VDP_LOG_JSON")
        .env_remove("VDP_PROJECT_DIR")
        .env("VDP_NO_VAULT", "1")
        .output()
        .unwrap()
}

fn assert_valid_ndjson_events(json_path: &Path) -> Vec<serde_json::Value> {
    let raw = std::fs::read_to_string(json_path)
        .unwrap_or_else(|e| panic!("expected NDJSON log at {}: {e}", json_path.display()));
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!lines.is_empty(), "expected at least one event in {raw}");
    lines
        .into_iter()
        .map(|line| {
            let event: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("invalid NDJSON line: {e}\n{line}"));
            let ts = event["timestamp"].as_str().unwrap_or("");
            let lvl = event["level"].as_str().unwrap_or("");
            let target = event["target"].as_str().unwrap_or("");
            let msg = event["message"].as_str().unwrap_or("");
            assert!(
                ts.ends_with('Z') && !ts.is_empty(),
                "bad timestamp: {event}"
            );
            assert!(!lvl.is_empty(), "missing level: {event}");
            assert!(!target.is_empty(), "missing target: {event}");
            assert!(!msg.is_empty(), "missing message: {event}");
            event
        })
        .collect()
}

#[test]
fn help_documents_log_json_level_policy() {
    let dir = tempfile::tempdir().unwrap();
    let out = vdp_cmd(dir.path(), &["--help"]);
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(
        help.contains("overwriting")
            && help.contains("info")
            && help.contains("debug")
            && help.contains("VDP_LOG"),
        "{help}"
    );
}

#[test]
fn env_show_default_stderr_is_empty_and_verbose_shows_human_service_milestone() {
    let dir = tempfile::tempdir().unwrap();
    let default_out = vdp_cmd(dir.path(), &["env", "show"]);
    assert_eq!(default_out.status.code(), Some(0));
    let default_err = String::from_utf8_lossy(&default_out.stderr);
    assert!(
        default_err.is_empty(),
        "expected empty stderr: {default_err}"
    );

    let verbose_out = vdp_cmd(dir.path(), &["-v", "env", "show"]);
    assert_eq!(verbose_out.status.code(), Some(0));
    let verbose_err = String::from_utf8_lossy(&verbose_out.stderr);
    assert!(
        verbose_err.contains("service initialized") && verbose_err.contains("secrets"),
        "stderr: {verbose_err}"
    );
    assert!(
        !verbose_err.contains("Arc<") && !verbose_err.contains("ResolvedSecrets"),
        "stderr must not expose Rust type names: {verbose_err}"
    );
}

#[test]
fn unwritable_path_emits_one_startup_warning_and_keeps_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let bad_path = dir.path().join("no_such_dir").join("run.json");

    let ok_out = vdp_cmd(
        dir.path(),
        &["--log-json", bad_path.to_str().unwrap(), "env", "show"],
    );
    assert_eq!(ok_out.status.code(), Some(0));
    let ok_err = String::from_utf8_lossy(&ok_out.stderr);
    let warnings: Vec<&str> = ok_err
        .lines()
        .filter(|l| l.contains("warning: could not open JSON run log"))
        .collect();
    assert_eq!(warnings.len(), 1, "{ok_err}");

    let missing_cfg = dir.path().join("missing.toml");
    let fail_out = vdp_cmd(
        dir.path(),
        &[
            "--log-json",
            bad_path.to_str().unwrap(),
            "--config",
            missing_cfg.to_str().unwrap(),
            "env",
            "show",
        ],
    );
    assert_eq!(fail_out.status.code(), Some(2));
    let fail_err = String::from_utf8_lossy(&fail_out.stderr);
    assert!(
        fail_err.contains("warning: could not open JSON run log"),
        "{fail_err}"
    );
    assert!(fail_err.contains("vdp: error:"), "{fail_err}");
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::path::PathBuf;

    use super::*;

    /// A `git` that answers what `vdp release analyse` asks and copies the run log aside when it is asked for the
    /// history (`MID_RUN_COPY`), which happens after the first `INFO` event and before the run ends. With `fail_log`
    /// that history request fails, so the run fails. It uses shell built-ins only: `PATH` holds nothing else.
    fn fake_git_dir(dir: &Path, fail_log: bool) -> PathBuf {
        let bin_dir = dir.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let fake_git = bin_dir.join("git");
        let fail = i32::from(fail_log);
        std::fs::write(
            &fake_git,
            format!(
                "#!/bin/sh\nset -e\ncase \" $* \" in\n  *\" rev-parse \"*|*\" merge-base \"*) echo 0123456789abcdef0123456789abcdef01234567 ;;\n  *\" log \"*)\n    if [ -n \"${{MID_RUN_COPY:-}}\" ]; then\n      while IFS= read -r line || [ -n \"$line\" ]; do\n        printf '%s\\n' \"$line\"\n      done < \"$LOG_UNDER_TEST\" > \"$MID_RUN_COPY\"\n    fi\n    exit {fail} ;;\nesac\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake_git, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin_dir
    }

    fn release_cmd(dir: &Path, bin_dir: &Path, extra_args: &[&str]) -> Command {
        let mut cmd = Command::new(VDP);
        cmd.args(extra_args)
            .args(["release", "analyse", "--fixversion", "2026_DROP05"])
            .current_dir(dir)
            .env("PATH", bin_dir)
            .env_remove("DBT_DEPLOY_TARGET")
            .env_remove("VDP_CONFIG")
            .env_remove("VDP_LOG")
            .env_remove("VDP_LOG_JSON")
            .env_remove("VDP_PROJECT_DIR")
            .env("VDP_NO_VAULT", "1");
        cmd
    }

    #[test]
    fn success_run_writes_valid_ndjson_via_flag_and_env() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = fake_git_dir(dir.path(), false);
        let flag_log = dir.path().join("flag.json");
        let out = release_cmd(
            dir.path(),
            &bin_dir,
            &["--log-json", flag_log.to_str().unwrap()],
        )
        .output()
        .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let events = assert_valid_ndjson_events(&flag_log);
        assert!(
            events.iter().any(|e| {
                e["level"] == "INFO"
                    && e["message"]
                        .as_str()
                        .unwrap_or("")
                        .contains("merge-base of")
            }),
            "{events:?}"
        );
        let mode = std::fs::metadata(&flag_log).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected mode 0600, got {mode:o}");

        let env_log = dir.path().join("env.json");
        // Pre-existing file must be overwritten cleanly.
        std::fs::write(&env_log, "stale\n").unwrap();
        let env_out = release_cmd(dir.path(), &bin_dir, &[])
            .env("VDP_LOG_JSON", &env_log)
            .output()
            .unwrap();
        assert_eq!(env_out.status.code(), Some(0));
        let overwritten = assert_valid_ndjson_events(&env_log);
        assert!(
            !std::fs::read_to_string(&env_log).unwrap().contains("stale"),
            "{overwritten:?}"
        );
    }

    #[test]
    fn dev_null_log_json_emits_no_warning_and_keeps_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let out = vdp_cmd(
            dir.path(),
            &["-v", "--log-json", "/dev/null", "env", "show"],
        );
        assert_eq!(out.status.code(), Some(0));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("warning:"), "stderr: {stderr}");
    }

    #[test]
    fn failing_run_still_writes_ndjson_and_keeps_exit_2() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = fake_git_dir(dir.path(), true);
        let fail_log = dir.path().join("fail.json");
        let out = release_cmd(dir.path(), &bin_dir, &[])
            .env("VDP_LOG_JSON", &fail_log)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        let events = assert_valid_ndjson_events(&fail_log);
        assert!(
            events.iter().any(|e| {
                e["level"] == "INFO"
                    && e["message"]
                        .as_str()
                        .unwrap_or("")
                        .contains("merge-base of")
            }),
            "{events:?}"
        );
    }

    #[test]
    fn events_are_streamed_to_disk_mid_run_before_guard_drops() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = fake_git_dir(dir.path(), false);
        let log_path = dir.path().join("stream.json");
        let mid_copy = dir.path().join("mid_run.json");
        let out = release_cmd(
            dir.path(),
            &bin_dir,
            &["-v", "--log-json", log_path.to_str().unwrap()],
        )
        .env("LOG_UNDER_TEST", &log_path)
        .env("MID_RUN_COPY", &mid_copy)
        .output()
        .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mid_events = assert_valid_ndjson_events(&mid_copy);
        assert!(
            mid_events.iter().any(|e| {
                e["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("merge-base of")
            }),
            "{mid_events:?}"
        );
        assert!(
            !mid_events.iter().any(|e| e["message"]
                .as_str()
                .unwrap_or("")
                .contains("command completed")),
            "mid-run copy must precede runtime completion: {mid_events:?}"
        );
    }

    #[test]
    fn file_sink_level_policy_respects_default_verbose_quiet_and_vdp_log() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = fake_git_dir(dir.path(), false);

        // Default run: INFO+ only, no DEBUG events.
        let default_log = dir.path().join("default.json");
        let out = release_cmd(
            dir.path(),
            &bin_dir,
            &["--log-json", default_log.to_str().unwrap()],
        )
        .output()
        .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let default_events = assert_valid_ndjson_events(&default_log);
        assert!(default_events.iter().any(|e| e["level"] == "INFO"));
        assert!(
            default_events.iter().all(|e| e["level"] != "DEBUG"),
            "default file log must exclude debug events: {default_events:?}"
        );

        // Verbose (-v) run: captures DEBUG+ in addition to INFO+.
        let verbose_log = dir.path().join("verbose.json");
        let out = release_cmd(
            dir.path(),
            &bin_dir,
            &["-v", "--log-json", verbose_log.to_str().unwrap()],
        )
        .output()
        .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let verbose_events = assert_valid_ndjson_events(&verbose_log);
        assert!(verbose_events.iter().any(|e| e["level"] == "INFO"));
        assert!(
            verbose_events.iter().any(|e| {
                e["level"] == "DEBUG"
                    && e["message"]
                        .as_str()
                        .unwrap_or("")
                        .contains("service initialized")
            }),
            "{verbose_events:?}"
        );

        // Quiet (-q) run: stderr hides INFO, while the file still captures INFO+ (and no DEBUG).
        let quiet_log = dir.path().join("quiet.json");
        let out = release_cmd(
            dir.path(),
            &bin_dir,
            &["-q", "--log-json", quiet_log.to_str().unwrap()],
        )
        .output()
        .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let quiet_err = String::from_utf8_lossy(&out.stderr);
        assert!(!quiet_err.contains("merge-base of"), "{quiet_err}");
        let quiet_events = assert_valid_ndjson_events(&quiet_log);
        assert!(quiet_events.iter().any(|e| e["level"] == "INFO"));
        assert!(quiet_events.iter().all(|e| e["level"] != "DEBUG"));

        // $VDP_LOG overrides both stderr and the file sink.
        let override_log = dir.path().join("override.json");
        let out = release_cmd(
            dir.path(),
            &bin_dir,
            &["-q", "--log-json", override_log.to_str().unwrap()],
        )
        .env("VDP_LOG", "debug")
        .output()
        .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let override_err = String::from_utf8_lossy(&out.stderr);
        assert!(
            override_err.contains("service initialized"),
            "{override_err}"
        );
        let override_events = assert_valid_ndjson_events(&override_log);
        assert!(override_events.iter().any(|e| e["level"] == "DEBUG"));
    }

    #[test]
    fn dbt_multicall_with_vdp_log_json_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let dbt_link = dir.path().join("dbt");
        symlink(VDP, &dbt_link).unwrap();
        let dbt_log = dir.path().join("dbt.json");

        let _out = Command::new(&dbt_link)
            .arg("--version")
            .current_dir(dir.path())
            .env("PATH", dir.path())
            .env_remove("VDP_CONFIG")
            .env("VDP_NO_VAULT", "1")
            .env("VDP_LOG_JSON", &dbt_log)
            .output()
            .unwrap();
        assert!(
            !dbt_log.exists(),
            "dbt multicall mode must never write {}",
            dbt_log.display()
        );
    }

    #[test]
    fn dbt_multicall_with_vdp_log_debug_emits_no_runtime_lifecycle_lines() {
        let dir = tempfile::tempdir().unwrap();
        let link_dir = dir.path().join("link");
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&link_dir).unwrap();
        std::fs::create_dir_all(&bin_dir).unwrap();

        let dbt_link = link_dir.join("dbt");
        symlink(VDP, &dbt_link).unwrap();
        let fake_dbt = bin_dir.join("dbt");
        std::fs::write(&fake_dbt, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&fake_dbt, std::fs::Permissions::from_mode(0o755)).unwrap();
        let dbt_log = dir.path().join("dbt.json");

        let out = Command::new(&dbt_link)
            .arg("--version")
            .current_dir(dir.path())
            .env("PATH", &bin_dir)
            .env_remove("DBT_DEPLOY_TARGET")
            .env_remove("VDP_CONFIG")
            .env("VDP_NO_VAULT", "1")
            .env("VDP_LOG", "debug")
            .env("VDP_LOG_JSON", &dbt_log)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "stderr: {stderr}");
        assert!(!dbt_log.exists());
        assert!(!stderr.contains("command started"), "stderr: {stderr}");
        assert!(!stderr.contains("command completed"), "stderr: {stderr}");
        assert!(!stderr.contains("command failed"), "stderr: {stderr}");

        let wrapper_out = Command::new(VDP)
            .args(["dbt", "--version"])
            .current_dir(dir.path())
            .env("PATH", &bin_dir)
            .env_remove("DBT_DEPLOY_TARGET")
            .env_remove("VDP_CONFIG")
            .env("VDP_NO_VAULT", "1")
            .env("VDP_LOG", "debug")
            .output()
            .unwrap();
        let wrapper_err = String::from_utf8_lossy(&wrapper_out.stderr);
        assert!(wrapper_out.status.success(), "stderr: {wrapper_err}");
        assert!(
            !wrapper_err.contains("command started"),
            "stderr: {wrapper_err}"
        );
        assert!(
            !wrapper_err.contains("command completed"),
            "stderr: {wrapper_err}"
        );

        let verbose_out = vdp_cmd(dir.path(), &["-v", "env", "show"]);
        let verbose_err = String::from_utf8_lossy(&verbose_out.stderr);
        assert!(verbose_out.status.success(), "stderr: {verbose_err}");
        assert!(
            verbose_err.contains("command started"),
            "stderr: {verbose_err}"
        );
        assert!(
            verbose_err.contains("command completed"),
            "stderr: {verbose_err}"
        );

        let multicall_run = Command::new(&dbt_link)
            .args(["run", "-s", "my_model"])
            .current_dir(dir.path())
            .env("PATH", &bin_dir)
            .env_remove("DBT_DEPLOY_TARGET")
            .env_remove("VDP_CONFIG")
            .env("VDP_NO_VAULT", "1")
            .env("VDP_LOG", "debug")
            .env("VDP_LOG_JSON", &dbt_log)
            .output()
            .unwrap();
        let multicall_run_err = String::from_utf8_lossy(&multicall_run.stderr);
        assert!(
            multicall_run.status.success(),
            "stderr: {multicall_run_err}"
        );
        assert!(!dbt_log.exists());
        assert!(
            !multicall_run_err.contains("milestone")
                && !multicall_run_err.contains("service initialized")
                && !multicall_run_err.contains("command started"),
            "stderr: {multicall_run_err}"
        );

        let wrapper_run = Command::new(VDP)
            .args(["dbt", "run", "-s", "my_model"])
            .current_dir(dir.path())
            .env("PATH", &bin_dir)
            .env_remove("DBT_DEPLOY_TARGET")
            .env_remove("VDP_CONFIG")
            .env("VDP_NO_VAULT", "1")
            .env("VDP_LOG", "debug")
            .output()
            .unwrap();
        let wrapper_run_err = String::from_utf8_lossy(&wrapper_run.stderr);
        assert!(wrapper_run.status.success(), "stderr: {wrapper_run_err}");
        assert!(
            !wrapper_run_err.contains("milestone")
                && !wrapper_run_err.contains("service initialized")
                && !wrapper_run_err.contains("command started"),
            "stderr: {wrapper_run_err}"
        );
    }
}

#[test]
fn env_show_with_vault_token_and_no_role_id_skips_vault_without_warning() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(VDP)
        .args(["env", "show"])
        .current_dir(dir.path())
        .env("VAULT_TOKEN", "dummy")
        .env_remove("KEYVAULT_ROLE_ID")
        .env_remove("KEYVAULT_ROLE_ID_LDV_ABT")
        .env_remove("KEYVAULT_SECRET_ID")
        .env_remove("KEYVAULT_SECRET_ID_LDV_ABT")
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_LOG")
        .env_remove("VDP_LOG_JSON")
        .env_remove("VDP_NO_VAULT")
        .env_remove("VDP_PROJECT_DIR")
        .output()
        .unwrap();

    let err = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "stderr: {err}");
    assert!(err.is_empty(), "expected empty stderr, got: {err}");
    assert!(
        stdout.contains(
            "vault status    skipped: no AppRole role id (KEYVAULT_ROLE_ID or KEYVAULT_ROLE_ID_LDV_ABT)"
        ),
        "stdout:\n{stdout}"
    );
}
