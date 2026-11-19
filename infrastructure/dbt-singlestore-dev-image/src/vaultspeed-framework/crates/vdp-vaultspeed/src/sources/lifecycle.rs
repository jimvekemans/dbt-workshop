use std::collections::BTreeSet;
use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};

use vdp_vaultspeed_domain::{RelPath, RunMode, SourceKind};
use walkdir::WalkDir;

use crate::sources::{DiscoveredSource, SourceError, SourceLocation};

/// How a source fared, as far as housekeeping is concerned.
#[derive(Debug, Clone)]
pub struct SourceOutcome<'a> {
    pub source: &'a DiscoveredSource,
    /// No statement failed, nothing was halted and nothing else went wrong.
    pub succeeded: bool,
    /// Files (relative to the source root) whose groups all completed: executed without
    /// failure or legitimately skipped. Anything not listed (failed, halted, unreadable)
    /// is treated as not done.
    pub completed_files: BTreeSet<RelPath>,
}

/// What happens to a source's staged files after processing.
pub trait SourceLifecycle: Send + Sync + Debug {
    /// Whether the lifecycle handles sources of `kind`.
    fn supports(&self, kind: &SourceKind) -> bool;

    /// Returns human-readable notes for the report. Must not touch anything in preview.
    ///
    /// # Errors
    ///
    /// [`SourceError::Io`] when a file cannot be read or written while moving or removing files.
    fn complete(
        &self,
        outcome: &SourceOutcome<'_>,
        mode: RunMode,
    ) -> Result<Vec<String>, SourceError>;
}

/// Moves each completed PML script to `deployed/PML`, overwriting the previous version.
/// Scripts that failed, were never reached (halted run) or could not be parsed stay
/// staged, so the next run retries them.
#[derive(Debug, Clone)]
pub struct PmlLifecycle {
    deployed_dir: PathBuf,
}

impl PmlLifecycle {
    /// Creates the lifecycle that moves completed scripts into `deployed_pml_dir`.
    pub const fn new(deployed_pml_dir: PathBuf) -> Self {
        Self {
            deployed_dir: deployed_pml_dir,
        }
    }
}

impl SourceLifecycle for PmlLifecycle {
    fn supports(&self, kind: &SourceKind) -> bool {
        matches!(kind, SourceKind::Pml)
    }

    fn complete(
        &self,
        outcome: &SourceOutcome<'_>,
        mode: RunMode,
    ) -> Result<Vec<String>, SourceError> {
        let SourceLocation::Directory(staging) = &outcome.source.location else {
            return Ok(Vec::new());
        };
        let mut notes = Vec::new();
        for entry in WalkDir::new(staging)
            .sort_by_file_name()
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !entry.file_type().is_file()
                || !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("sql"))
            {
                continue;
            }
            let rel = RelPath::new(path.strip_prefix(staging).unwrap_or(path).to_string_lossy());
            if !outcome.completed_files.contains(&rel) {
                notes.push(format!(
                    "kept PML script {rel} in staging: it did not complete"
                ));
                continue;
            }
            let target = self.deployed_dir.join(rel.as_str());
            if mode.is_preview() {
                notes.push(format!(
                    "would move PML script {rel} to {}",
                    target.display()
                ));
                continue;
            }
            move_file(path, &target)?;
            notes.push(format!("moved PML script {rel} to {}", target.display()));
        }
        Ok(notes)
    }
}

/// Moves processed generation zips to `deployed/archive`.
///
/// With `archive_on_failure = false` a zip with failures stays in staging, so the next
/// pipeline retries it; the ledger makes the retry skip everything that already ran.
#[derive(Debug, Clone)]
pub struct ZipArchiveLifecycle {
    archive_dir: PathBuf,
    archive_on_failure: bool,
}

impl ZipArchiveLifecycle {
    /// Creates the lifecycle that moves processed zips into `archive_dir`; with `archive_on_failure` also zips with failures.
    pub const fn new(archive_dir: PathBuf, archive_on_failure: bool) -> Self {
        Self {
            archive_dir,
            archive_on_failure,
        }
    }
}

impl SourceLifecycle for ZipArchiveLifecycle {
    fn supports(&self, kind: &SourceKind) -> bool {
        matches!(kind, SourceKind::Generation(_))
    }

    fn complete(
        &self,
        outcome: &SourceOutcome<'_>,
        mode: RunMode,
    ) -> Result<Vec<String>, SourceError> {
        let SourceLocation::Zip(zip) = &outcome.source.location else {
            return Ok(Vec::new());
        };
        let name = zip.file_name().unwrap_or_default();
        if !outcome.succeeded && !self.archive_on_failure {
            return Ok(vec![format!(
                "kept {} in staging because of failures; the next run retries it",
                name.to_string_lossy()
            )]);
        }
        let target = self.archive_dir.join(name);
        if mode.is_preview() {
            return Ok(vec![format!(
                "would archive {} to {}",
                zip.display(),
                target.display()
            )]);
        }
        if target.exists() {
            fs::remove_file(zip).map_err(SourceError::io(zip))?;
            return Ok(vec![format!(
                "{} was already archived; removed the staged duplicate",
                name.to_string_lossy()
            )]);
        }
        move_file(zip, &target)?;
        Ok(vec![format!(
            "archived {} to {}",
            name.to_string_lossy(),
            target.display()
        )])
    }
}

/// `rename`, falling back to copy + delete across file systems.
fn move_file(from: &Path, to: &Path) -> Result<(), SourceError> {
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir).map_err(SourceError::io(dir))?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).map_err(SourceError::io(to))?;
    fs::remove_file(from).map_err(SourceError::io(from))
}

#[cfg(test)]
mod tests {
    use vdp_vaultspeed_domain::Priority;

    use super::*;

    fn pml_source(dir: &Path) -> DiscoveredSource {
        DiscoveredSource {
            kind: SourceKind::Pml,
            priority: Priority::HIGHEST,
            location: SourceLocation::Directory(dir.to_owned()),
        }
    }

    #[test]
    fn pml_moves_successes_keeps_failures_and_never_touches_preview() {
        let staging = tempfile::tempdir().unwrap();
        let deployed = tempfile::tempdir().unwrap();
        fs::write(staging.path().join("ok_script.sql"), "SELECT 1").unwrap();
        fs::write(staging.path().join("Failing_Script.sql"), "SELEC 1").unwrap();
        let source = pml_source(staging.path());
        let lifecycle = PmlLifecycle::new(deployed.path().to_owned());
        fs::write(staging.path().join("never_reached.sql"), "SELECT 2").unwrap();
        let outcome = SourceOutcome {
            source: &source,
            succeeded: false,
            completed_files: BTreeSet::from([RelPath::new("ok_script.sql")]),
        };

        lifecycle.complete(&outcome, RunMode::Preview).unwrap();
        assert!(
            staging.path().join("ok_script.sql").exists(),
            "preview must not move files"
        );

        lifecycle.complete(&outcome, RunMode::Apply).unwrap();
        assert!(!staging.path().join("ok_script.sql").exists());
        assert!(deployed.path().join("ok_script.sql").exists());
        assert!(
            staging.path().join("Failing_Script.sql").exists(),
            "failed scripts stay staged"
        );
        assert!(
            staging.path().join("never_reached.sql").exists(),
            "unexecuted scripts stay staged"
        );
    }

    #[test]
    fn zip_with_failures_stays_staged_unless_configured() {
        let staging = tempfile::tempdir().unwrap();
        let archive = tempfile::tempdir().unwrap();
        let zip = staging.path().join("12_DDL.zip");
        fs::write(&zip, "zip").unwrap();
        let source = DiscoveredSource {
            kind: SourceKind::Generation("12_DDL".parse().unwrap()),
            priority: Priority::new(1012),
            location: SourceLocation::Zip(zip.clone()),
        };
        let failed = SourceOutcome {
            source: &source,
            succeeded: false,
            completed_files: BTreeSet::new(),
        };

        ZipArchiveLifecycle::new(archive.path().to_owned(), false)
            .complete(&failed, RunMode::Apply)
            .unwrap();
        assert!(zip.exists());

        ZipArchiveLifecycle::new(archive.path().to_owned(), true)
            .complete(&failed, RunMode::Apply)
            .unwrap();
        assert!(!zip.exists());
        assert!(archive.path().join("12_DDL.zip").exists());
    }
}
