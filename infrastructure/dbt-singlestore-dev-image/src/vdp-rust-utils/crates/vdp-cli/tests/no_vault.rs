//! `$VDP_NO_VAULT` is one boolean for `vdp` and for the `dbt` link.
#![cfg(unix)]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

/// A command in `dir` with an empty environment (no AppRole ids, so Vault is never really called) plus `VDP_NO_VAULT`.
fn command(program: &Path, dir: &Path, no_vault: &str) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env_clear()
        .env("VDP_NO_VAULT", no_vault);
    command
}

/// What `vdp env doctor` says about Vault.
fn vault_line(dir: &Path, no_vault: &str) -> String {
    let out = command(Path::new(VDP), dir, no_vault)
        .args(["env", "doctor"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    stdout
        .lines()
        .find(|line| line.contains(" vault "))
        .unwrap_or_else(|| {
            panic!(
                "no vault line for {no_vault:?}:\n{stdout}\n{}",
                String::from_utf8_lossy(&out.stderr)
            )
        })
        .to_owned()
}

#[test]
fn vdp_reads_vdp_no_vault_as_a_boolean() {
    let dir = tempfile::tempdir().unwrap();
    for on in ["1", "true", "yes", "on", "TRUE"] {
        assert!(
            vault_line(dir.path(), on).ends_with("skipped: --no-vault"),
            "{on}"
        );
    }
    // Vault stays on: with no AppRole id it is skipped for that reason, not for `--no-vault`.
    for off in ["", "0", "false", "no", "off", "OFF"] {
        let line = vault_line(dir.path(), off);
        assert!(
            line.contains("skipped: no AppRole role id"),
            "{off:?}: {line}"
        );
    }
}

#[test]
fn vdp_refuses_vdp_no_vault_that_is_not_a_boolean() {
    let dir = tempfile::tempdir().unwrap();
    let out = command(Path::new(VDP), dir.path(), "maybe")
        .args(["env", "doctor"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("`maybe` is not a boolean") && stderr.contains("--no-vault"),
        "stderr: {stderr}"
    );
}

/// `dbt --version` through a link to `vdp`, with a stand-in engine that succeeds.
fn link_version(dir: &Path, no_vault: &str) -> Output {
    let link_dir = dir.join("link");
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&link_dir).unwrap();
    std::fs::create_dir_all(&bin_dir).unwrap();
    let link = link_dir.join("dbt");
    if !link.exists() {
        symlink(VDP, &link).unwrap();
        let engine = bin_dir.join("dbt");
        std::fs::write(&engine, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    command(&link, dir, no_vault)
        .env("PATH", &bin_dir)
        .arg("--version")
        .output()
        .unwrap()
}

#[test]
fn the_dbt_link_accepts_the_same_values_as_vdp() {
    let dir = tempfile::tempdir().unwrap();
    for value in ["", "1", "true", "yes", "on", "0", "false", "no", "off"] {
        let out = link_version(dir.path(), value);
        assert!(
            out.status.success(),
            "{value:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn the_dbt_link_refuses_vdp_no_vault_that_is_not_a_boolean() {
    let dir = tempfile::tempdir().unwrap();
    let out = link_version(dir.path(), "maybe");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("$VDP_NO_VAULT: `maybe` is not a boolean"),
        "stderr: {stderr}"
    );
}
