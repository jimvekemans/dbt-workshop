//! The `vdp-release-ui` binary as a process: `release serve`, and the commands of `vdp` it carries along.

use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_vdp-release-ui");

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env_remove("DBT_DEPLOY_TARGET")
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_LOG")
        .env_remove("VDP_LOG_JSON")
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

#[test]
fn release_serve_prints_listening_address_even_with_quiet_flag() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("vdp.toml");
    std::fs::write(&config_path, "[release_hub]\nauth = \"none\"\n").unwrap();

    let mut child = Command::new(BIN)
        .args([
            "-q",
            "--no-vault",
            "release",
            "serve",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--repo-dir",
            dir.path().to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .env("VDP_CONFIG", &config_path)
        .env_remove("VDP_LOG")
        .env_remove("VDP_LOG_JSON")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader_handle = std::thread::spawn(move || {
        let mut lines = BufReader::new(stderr).lines();
        while let Some(Ok(line)) = lines.next() {
            if line.starts_with("release hub on ") {
                let _ = tx.send(line);
                break;
            }
        }
    });

    let line = rx.recv_timeout(Duration::from_secs(3));
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader_handle.join();

    let line = line.expect("expected 'release hub on ...' line on stderr even under -q");
    assert!(
        line.starts_with("release hub on http://127.0.0.1:")
            && line.contains("/api/v1 (repository "),
        "unexpected hub banner line: {line}"
    );
}

#[test]
fn release_serve_help_lists_the_hub_options() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["release", "serve", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let help = stdout(&out);
    for option in [
        "--repo-dir",
        "--bind",
        "--port",
        "--ui-dir",
        "--cors-origin",
        "--validate",
    ] {
        assert!(help.contains(option), "`{option}` is missing from:\n{help}");
    }
}

#[test]
fn the_release_commands_of_vdp_work_next_to_serve() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["release", "analyse", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("--fixversion"), "{}", stdout(&out));

    let out = run(dir.path(), &["release", "--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let help = stdout(&out);
    for command in ["create", "analyse", "patch", "promote", "branches", "serve"] {
        assert!(
            help.lines().any(|l| l.trim_start().starts_with(command)),
            "`{command}` is missing from:\n{help}"
        );
    }
}

#[test]
fn help_lists_the_commands_of_vdp() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["--help"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let help = stdout(&out);
    for command in ["dbt", "sources", "release", "jira", "sql", "qa", "env"] {
        assert!(
            help.lines().any(|l| l.trim_start().starts_with(command)),
            "`{command}` is missing from:\n{help}"
        );
    }
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
    // The hand-off of `vdp` to the other binaries is not a command of this one.
    let out = run(dir.path(), &["vaultspeed", "plan"]);
    assert_eq!(out.status.code(), Some(2));
}
