//! `[git] timeout_secs`: the command line stops a git call that hangs.
#![cfg(all(unix, feature = "release"))]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

/// `vdp release analyse` in `dir` with `config` as `vdp.toml` and a `git` that never finishes (a loop of shell built-ins:
/// `PATH` holds nothing else).
fn analyse_with_hanging_git(dir: &Path, config: &str) -> (Output, Duration) {
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let git = bin_dir.join("git");
    std::fs::write(&git, "#!/bin/sh\nwhile :; do :; done\n").unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config_file = dir.join("vdp.toml");
    std::fs::write(&config_file, config).unwrap();
    let started = Instant::now();
    let out = Command::new(VDP)
        .arg("--config")
        .arg(&config_file)
        .args(["release", "analyse", "--fixversion", "2026_DROP05"])
        .current_dir(dir)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("VDP_NO_VAULT", "1")
        .output()
        .unwrap();
    (out, started.elapsed())
}

#[test]
fn a_hanging_git_is_stopped_after_the_configured_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let (out, took) = analyse_with_hanging_git(dir.path(), "[git]\ntimeout_secs = 1\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("did not finish within 1 s and was stopped"),
        "stderr: {stderr}"
    );
    assert!(took < Duration::from_secs(30), "took {took:?}");
}

#[test]
fn an_unknown_git_key_is_refused_with_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let (out, _) = analyse_with_hanging_git(dir.path(), "[git]\ntimeout = 1\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("unknown field `timeout`"),
        "stderr: {stderr}"
    );
}
