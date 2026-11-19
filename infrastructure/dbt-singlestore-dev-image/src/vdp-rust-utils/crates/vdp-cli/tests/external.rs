//! `vdp vaultspeed ...` and `vdp release-ui ...` hand over to the separate `vdp-vaultspeed` and `vdp-release-ui`
//! binaries; other unknown commands stay errors.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

/// The built `vdp`, linked or copied into a directory of its own, and a separate directory for `PATH`.
///
/// `vdp` looks for the other binaries next to itself first and on `PATH` after that. A `vdp-vaultspeed` or
/// `vdp-release-ui` that happens to sit beside the built `vdp` (a full `cargo build` of the toolkit leaves them there)
/// would be found before anything a test arranges, so the tests run that copy instead: its directory holds nothing
/// else, and `PATH` is exactly the directory the test fills (empty unless it installs a fake binary).
struct Sandbox {
    vdp: PathBuf,
    path: tempfile::TempDir,
    _bin: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        // Next to the built `vdp` (same file system), so the copy can be a hard link: the debug binary is large and
        // every test makes one. A symbolic link would not do, `current_exe` resolves it to the original directory.
        let bin = tempfile::Builder::new()
            .prefix("vdp-sandbox-")
            .tempdir_in(Path::new(VDP).parent().unwrap())
            .unwrap();
        let vdp = bin
            .path()
            .join(format!("vdp{}", std::env::consts::EXE_SUFFIX));
        if std::fs::hard_link(VDP, &vdp).is_err() {
            std::fs::copy(VDP, &vdp).unwrap();
        }
        Self {
            vdp,
            path: tempfile::tempdir().unwrap(),
            _bin: bin,
        }
    }

    /// Runs the copied `vdp` with only this sandbox's `PATH` directory on `PATH`.
    fn vdp(&self, args: &[&str]) -> Output {
        Command::new(&self.vdp)
            .args(args)
            .env("PATH", self.path.path())
            .env_remove("VDP_CONFIG")
            .output()
            .unwrap()
    }
}

#[test]
fn a_missing_framework_binary_is_exit_2_with_an_explanation() {
    let out = Sandbox::new().vdp(&["vaultspeed", "plan"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("separate binary"), "{stderr}");
    assert!(stderr.contains("vdp-vaultspeed"), "{stderr}");
    assert!(stderr.contains("vaultspeed-framework"), "{stderr}");
}

#[test]
fn a_missing_release_ui_binary_is_exit_2_with_an_explanation() {
    let out = Sandbox::new().vdp(&["release-ui", "release", "serve"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("separate binary"), "{stderr}");
    assert!(stderr.contains("`vdp release-ui`"), "{stderr}");
    assert!(stderr.contains("vdp-release-ui"), "{stderr}");
    assert!(stderr.contains("release-ui workspace"), "{stderr}");
}

#[test]
fn an_unknown_subcommand_is_clap_s_error() {
    let out = Sandbox::new().vdp(&["frobnicate"]);
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

    fn fake_binary(dir: &Path, name: &str, exit: i32) -> PathBuf {
        let path = dir.join(name);
        // One argument per line, so the test sees exactly what arrived.
        std::fs::write(
            &path,
            format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done\nexit {exit}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Runs `vdp args` with only a fake `binary` on PATH: its exit code and the arguments it received.
    fn forwarded(binary: &str, args: &[&str], exit: i32) -> (Option<i32>, Vec<String>) {
        let sandbox = Sandbox::new();
        fake_binary(sandbox.path.path(), binary, exit);
        let out = sandbox.vdp(args);
        let lines = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect();
        (out.status.code(), lines)
    }

    #[test]
    fn the_original_arguments_are_passed_on_unchanged() {
        let (code, lines) = forwarded(
            "vdp-vaultspeed",
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
        let (_, lines) = forwarded(
            "vdp-vaultspeed",
            &["--env", "LDV", "-v", "vaultspeed", "plan"],
            0,
        );
        assert_eq!(lines, ["--env", "LDV", "-v", "vaultspeed", "plan"]);
    }

    #[test]
    fn the_exit_code_of_the_framework_binary_is_the_exit_code() {
        let (code, _) = forwarded("vdp-vaultspeed", &["vaultspeed", "deploy"], 7);
        assert_eq!(code, Some(7));
    }

    #[test]
    fn release_ui_help_is_forwarded_without_the_command_name() {
        let (code, lines) = forwarded("vdp-release-ui", &["release-ui", "--help"], 0);
        assert_eq!(code, Some(0));
        assert_eq!(lines, ["--help"]);
    }

    #[test]
    fn release_ui_gets_its_arguments_with_the_global_options_kept_in_place() {
        let (code, lines) = forwarded(
            "vdp-release-ui",
            &[
                "--env",
                "LDV",
                "-q",
                "release-ui",
                "release",
                "serve",
                "--port",
                "0",
            ],
            0,
        );
        assert_eq!(code, Some(0));
        assert_eq!(
            lines,
            ["--env", "LDV", "-q", "release", "serve", "--port", "0"]
        );
    }

    #[test]
    fn the_exit_code_of_the_release_ui_binary_is_the_exit_code() {
        let (code, _) = forwarded("vdp-release-ui", &["release-ui", "release", "serve"], 9);
        assert_eq!(code, Some(9));
    }

    #[test]
    fn each_command_runs_only_its_own_binary() {
        // Only the framework binary is installed: `release-ui` is not served by it.
        let (code, lines) = forwarded("vdp-vaultspeed", &["release-ui", "--help"], 0);
        assert_eq!(code, Some(2));
        assert_eq!(lines, Vec::<String>::new());
    }
}
