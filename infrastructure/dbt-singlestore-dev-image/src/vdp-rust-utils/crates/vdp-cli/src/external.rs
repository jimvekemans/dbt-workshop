//! Commands that belong to another binary of the toolkit.
//!
//! `vdp vaultspeed ...` is the VaultSpeed framework's command line, a separate binary (`vdp-vaultspeed`, built from
//! the vaultspeed-framework workspace) that carries the whole `vdp` command line as well. `vdp` does not know
//! the arguments of the command; it replaces itself with the framework binary and passes its own arguments on,
//! unchanged and in the same order, so global options before the command (`vdp --env LDV vaultspeed plan`) arrive
//! as written.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::CommandFactory as _;

use crate::args::Cli;

/// The binary behind `vdp vaultspeed`.
pub const VAULTSPEED_BINARY: &str = "vdp-vaultspeed";

/// The exit code of a hand-off that could not happen.
const NOT_FOUND: u8 = 2;

/// The binary that runs the external command `name`, if the toolkit knows one.
#[must_use]
pub fn binary_for(name: &OsStr) -> Option<&'static str> {
    (name == "vaultspeed").then_some(VAULTSPEED_BINARY)
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
    let Some(binary) = binary_for(name) else {
        reject(name)
    };
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let path = std::env::var_os("PATH");
    let Some(found) = find_binary(binary, exe_dir.as_deref(), path.as_deref()) else {
        eprintln!(
            "vdp: error: `vdp {0}` is the VaultSpeed framework, a separate binary (`{binary}`, built from the \
             vaultspeed-framework workspace); it was not found next to vdp or on PATH",
            name.to_string_lossy()
        );
        return ExitCode::from(NOT_FOUND);
    };
    hand_over(&found, argv)
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

    #[test]
    fn only_vaultspeed_is_known() {
        assert_eq!(binary_for(OsStr::new("vaultspeed")), Some("vdp-vaultspeed"));
        assert_eq!(binary_for(OsStr::new("sources")), None);
        assert_eq!(binary_for(OsStr::new("")), None);
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
