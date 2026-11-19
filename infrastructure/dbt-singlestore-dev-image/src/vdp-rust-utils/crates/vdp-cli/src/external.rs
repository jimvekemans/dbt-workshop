//! Commands that belong to another binary of the toolkit.
//!
//! `vdp` carries the toolkit's core. Two modules live in workspaces of their own, next to this one, and their binaries
//! carry the whole `vdp` command line as well (see [`HANDOFFS`]):
//!
//! - `vdp vaultspeed ...` is the VaultSpeed framework, `vdp-vaultspeed` (the vaultspeed-framework workspace);
//! - `vdp release-ui ...` is the release UI's backend, `vdp-release-ui` (the release-ui workspace), whose
//!   `release serve` starts the release hub.
//!
//! `vdp` does not know the arguments of these commands; it replaces itself with the other binary and passes its own
//! arguments on, in the same order, so global options before the command (`vdp --env LDV vaultspeed plan`) arrive as
//! written. The binary is `vdp-vaultspeed vaultspeed plan` for the first, whose command line has a `vaultspeed`
//! command of its own, and `vdp-release-ui release serve` for `vdp release-ui release serve`: the binary *is* the
//! release UI, so the command name is dropped.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::CommandFactory as _;

use crate::args::Cli;

/// A command of `vdp` that another binary of the toolkit runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handoff {
    /// The command after `vdp`: `vdp <command> ...`.
    pub command: &'static str,
    /// The binary that runs it, found next to `vdp` or on `PATH`.
    pub binary: &'static str,
    /// Whether the binary's own command line has a command of the same name (`vdp-vaultspeed vaultspeed ...`), so
    /// the arguments are passed on as they are. When it has not (`vdp-release-ui` is the command), the command name
    /// is dropped from the arguments.
    pub keeps_command: bool,
    /// What the command is, for the message when the binary is missing.
    pub what: &'static str,
    /// The workspace the binary is built from, for the same message.
    pub workspace: &'static str,
}

/// The commands of `vdp` that other binaries run.
pub const HANDOFFS: &[Handoff] = &[
    Handoff {
        command: "vaultspeed",
        binary: "vdp-vaultspeed",
        keeps_command: true,
        what: "the VaultSpeed framework",
        workspace: "vaultspeed-framework",
    },
    Handoff {
        command: "release-ui",
        binary: "vdp-release-ui",
        keeps_command: false,
        what: "the release UI's backend",
        workspace: "release-ui",
    },
];

/// The exit code of a hand-off that could not happen.
const NOT_FOUND: u8 = 2;

/// The hand-off for the external command `name`, if the toolkit knows one.
#[must_use]
pub fn handoff_for(name: &OsStr) -> Option<&'static Handoff> {
    HANDOFFS.iter().find(|handoff| name == handoff.command)
}

/// The arguments the binary of `handoff` receives: `argv` (the original arguments after the program name), without
/// the command name unless [`Handoff::keeps_command`].
///
/// `external` is the command with everything after it, as clap parsed it, so its length locates the command name in
/// `argv` however many global options come before it.
#[must_use]
pub fn forwarded_args(
    handoff: &Handoff,
    external: &[OsString],
    argv: &[OsString],
) -> Vec<OsString> {
    let mut forwarded = argv.to_vec();
    if !handoff.keeps_command
        && let Some(at) = argv.len().checked_sub(external.len())
        && argv.get(at).map(OsString::as_os_str) == Some(OsStr::new(handoff.command))
    {
        forwarded.remove(at);
    }
    forwarded
}

/// `binary` in `dir` (the running executable's directory), else on `path` (`$PATH`'s value).
#[must_use]
pub fn find_binary(binary: &str, dir: Option<&Path>, path: Option<&OsStr>) -> Option<PathBuf> {
    let file = format!("{binary}{}", std::env::consts::EXE_SUFFIX);
    dir.map(Path::to_path_buf)
        .into_iter()
        .chain(path.into_iter().flat_map(std::env::split_paths))
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
}

/// Runs the external command `external` (`[subcommand, args...]` as clap parsed it) by handing `argv` (the
/// original arguments after the program name) to its binary, and returns its exit code. On unix the process is
/// replaced and this only returns when that fails.
///
/// An unknown subcommand is reported the way clap reports one and exits with code 2. A known one whose binary
/// cannot be found says so and returns 2.
#[must_use]
pub fn run(external: &[OsString], argv: &[OsString]) -> ExitCode {
    let name = external
        .first()
        .map(OsString::as_os_str)
        .unwrap_or_default();
    let Some(handoff) = handoff_for(name) else {
        reject(name)
    };
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let path = std::env::var_os("PATH");
    let Some(found) = find_binary(handoff.binary, exe_dir.as_deref(), path.as_deref()) else {
        eprintln!(
            "vdp: error: `vdp {}` is {}, a separate binary (`{}`, built from the {} workspace); it was not \
             found next to vdp or on PATH",
            handoff.command, handoff.what, handoff.binary, handoff.workspace
        );
        return ExitCode::from(NOT_FOUND);
    };
    hand_over(&found, &forwarded_args(handoff, external, argv))
}

/// A clap error for an unknown subcommand, as clap reports one (it exits the process with code 2).
fn reject(name: &OsStr) -> ! {
    Cli::command()
        .error(
            clap::error::ErrorKind::InvalidSubcommand,
            format!("unrecognized subcommand '{}'", name.to_string_lossy()),
        )
        .exit()
}

#[cfg(unix)]
fn hand_over(binary: &Path, argv: &[OsString]) -> ExitCode {
    use std::os::unix::process::CommandExt as _;

    let error = std::process::Command::new(binary).args(argv).exec();
    eprintln!("vdp: error: cannot run {}: {error}", binary.display());
    ExitCode::from(NOT_FOUND)
}

#[cfg(not(unix))]
fn hand_over(binary: &Path, argv: &[OsString]) -> ExitCode {
    match std::process::Command::new(binary).args(argv).status() {
        Ok(status) => ExitCode::from(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1)),
        Err(error) => {
            eprintln!("vdp: error: cannot run {}: {error}", binary.display());
            ExitCode::from(NOT_FOUND)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn only_the_commands_of_the_table_are_known() {
        let vaultspeed = handoff_for(OsStr::new("vaultspeed")).unwrap();
        assert_eq!(vaultspeed.binary, "vdp-vaultspeed");
        let release_ui = handoff_for(OsStr::new("release-ui")).unwrap();
        assert_eq!(release_ui.binary, "vdp-release-ui");
        assert_eq!(handoff_for(OsStr::new("sources")), None);
        assert_eq!(handoff_for(OsStr::new("release")), None);
        assert_eq!(handoff_for(OsStr::new("")), None);
    }

    #[test]
    fn the_table_has_unique_commands_and_binaries() {
        for (i, a) in HANDOFFS.iter().enumerate() {
            for b in &HANDOFFS[i + 1..] {
                assert_ne!(a.command, b.command);
                assert_ne!(a.binary, b.binary);
            }
        }
    }

    #[test]
    fn vaultspeed_keeps_its_command_name_and_release_ui_drops_it() {
        let vaultspeed = handoff_for(OsStr::new("vaultspeed")).unwrap();
        let argv = os(&["--env", "LDV", "vaultspeed", "plan"]);
        assert_eq!(
            forwarded_args(vaultspeed, &os(&["vaultspeed", "plan"]), &argv),
            argv
        );
        let release_ui = handoff_for(OsStr::new("release-ui")).unwrap();
        let argv = os(&[
            "--env",
            "LDV",
            "-q",
            "release-ui",
            "release",
            "serve",
            "--port",
            "0",
        ]);
        assert_eq!(
            forwarded_args(
                release_ui,
                &os(&["release-ui", "release", "serve", "--port", "0"]),
                &argv
            ),
            os(&["--env", "LDV", "-q", "release", "serve", "--port", "0"])
        );
        // A global option whose value is the command name is not mistaken for it.
        let argv = os(&["--target", "release-ui", "release-ui", "--help"]);
        assert_eq!(
            forwarded_args(release_ui, &os(&["release-ui", "--help"]), &argv),
            os(&["--target", "release-ui", "--help"])
        );
    }

    #[test]
    fn the_directory_of_vdp_comes_before_path() {
        let near = tempfile::tempdir().unwrap();
        let far = tempfile::tempdir().unwrap();
        let name = format!("vdp-vaultspeed{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(far.path().join(&name), "").unwrap();
        let path = std::env::join_paths([far.path()]).unwrap();
        assert_eq!(
            find_binary("vdp-vaultspeed", Some(near.path()), Some(&path)),
            Some(far.path().join(&name))
        );
        std::fs::write(near.path().join(&name), "").unwrap();
        assert_eq!(
            find_binary("vdp-vaultspeed", Some(near.path()), Some(&path)),
            Some(near.path().join(&name))
        );
        assert_eq!(find_binary("vdp-vaultspeed", None, None), None);
    }
}
