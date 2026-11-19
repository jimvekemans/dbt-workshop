//! `vdp vaultspeed ...` hands over to the separate `vdp-vaultspeed` binary; other unknown commands stay errors.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

/// A `vdp-vaultspeed` next to this `vdp` would be found before the one on PATH.
fn vdp_has_a_neighbour() -> bool {
    Path::new(VDP).parent().is_some_and(|dir| {
        dir.join(format!("vdp-vaultspeed{}", std::env::consts::EXE_SUFFIX))
            .is_file()
    })
}

fn vdp(path: &Path, args: &[&str]) -> Output {
    Command::new(VDP)
        .args(args)
        .env("PATH", path)
        .env_remove("VDP_CONFIG")
        .output()
        .unwrap()
}

#[test]
fn a_missing_framework_binary_is_exit_2_with_an_explanation() {
    if vdp_has_a_neighbour() {
        return;
    }
    let empty = tempfile::tempdir().unwrap();
    let out = vdp(empty.path(), &["vaultspeed", "plan"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("separate binary"), "{stderr}");
    assert!(stderr.contains("vdp-vaultspeed"), "{stderr}");
    assert!(stderr.contains("vaultspeed-framework"), "{stderr}");
}

#[test]
fn an_unknown_subcommand_is_clap_s_error() {
    let empty = tempfile::tempdir().unwrap();
    let out = vdp(empty.path(), &["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unrecognized subcommand 'frobnicate'"),
        "{stderr}"
    );
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn fake_framework(dir: &Path, exit: i32) -> PathBuf {
        let path = dir.join("vdp-vaultspeed");
        // One argument per line, so the test sees exactly what arrived.
        std::fs::write(
            &path,
            format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done\nexit {exit}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn forwarded(args: &[&str], exit: i32) -> (Option<i32>, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        fake_framework(dir.path(), exit);
        let out = vdp(dir.path(), args);
        let lines = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        (out.status.code(), lines)
    }

    #[test]
    fn the_original_arguments_are_passed_on_unchanged() {
        if vdp_has_a_neighbour() {
            return;
        }
        let (code, lines) = forwarded(
            &["vaultspeed", "plan", "--offline", "--files", "1134_FMC"],
            0,
        );
        assert_eq!(code, Some(0));
        assert_eq!(
            lines,
            ["vaultspeed", "plan", "--offline", "--files", "1134_FMC"]
        );
    }

    #[test]
    fn global_options_before_the_command_are_kept_in_place() {
        if vdp_has_a_neighbour() {
            return;
        }
        let (_, lines) = forwarded(&["--env", "LDV", "-v", "vaultspeed", "plan"], 0);
        assert_eq!(lines, ["--env", "LDV", "-v", "vaultspeed", "plan"]);
    }

    #[test]
    fn the_exit_code_of_the_framework_binary_is_the_exit_code() {
        if vdp_has_a_neighbour() {
            return;
        }
        let (code, _) = forwarded(&["vaultspeed", "deploy"], 7);
        assert_eq!(code, Some(7));
    }
}
