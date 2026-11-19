use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use vdp_vaultspeed_domain::{GenerationName, Priority, SourceKind};
use walkdir::WalkDir;

use crate::sources::SourceError;

/// Where a source's files live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLocation {
    Zip(PathBuf),
    Directory(PathBuf),
}

/// A unit of work found in staging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSource {
    pub kind: SourceKind,
    pub priority: Priority,
    pub location: SourceLocation,
}

/// Finds sources of one kind. The engine merges all discoveries and orders the result
/// by [`Priority`]; adding a new flow means adding a discovery, not editing the engine.
pub trait SourceDiscovery: Send + Sync + Debug {
    /// The sources this discovery finds, unordered.
    ///
    /// # Errors
    ///
    /// [`SourceError::Io`] when the staging directory cannot be read; [`SourceError::InvalidGenerationName`] when a staged zip
    /// is not named like a generation; [`SourceError::NotStaged`] when a requested generation is not staged.
    fn discover(&self) -> Result<Vec<DiscoveredSource>, SourceError>;
}

/// Staged PML scripts form one source with the highest priority, present only when at
/// least one `.sql` file is staged: staging a script is what triggers its execution.
#[derive(Debug, Clone)]
pub struct PmlDiscovery {
    dir: PathBuf,
}

impl PmlDiscovery {
    /// Creates the discovery for the PML directory `dir`.
    pub const fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

impl SourceDiscovery for PmlDiscovery {
    fn discover(&self) -> Result<Vec<DiscoveredSource>, SourceError> {
        let has_scripts = self.dir.is_dir()
            && WalkDir::new(&self.dir)
                .into_iter()
                .filter_map(Result::ok)
                .any(|e| {
                    e.file_type().is_file()
                        && e.path()
                            .extension()
                            .is_some_and(|x| x.eq_ignore_ascii_case("sql"))
                });
        if !has_scripts {
            return Ok(Vec::new());
        }
        let kind = SourceKind::Pml;
        Ok(vec![DiscoveredSource {
            priority: kind.default_priority(),
            kind,
            location: SourceLocation::Directory(self.dir.clone()),
        }])
    }
}

/// Generation zips in `staging/release`, optionally restricted to named generations.
#[derive(Debug, Clone)]
pub struct ReleaseZipDiscovery {
    dir: PathBuf,
    only: Option<Vec<String>>,
}

impl ReleaseZipDiscovery {
    /// Creates the discovery for the directory `dir` of staged generation zips.
    pub const fn new(dir: PathBuf) -> Self {
        Self { dir, only: None }
    }

    /// Restricts discovery to these generation labels (`1134_FMC`); every requested
    /// label must be staged, otherwise discovery fails instead of silently skipping.
    #[must_use]
    pub fn only(mut self, labels: Vec<String>) -> Self {
        self.only = Some(labels);
        self
    }
}

impl SourceDiscovery for ReleaseZipDiscovery {
    fn discover(&self) -> Result<Vec<DiscoveredSource>, SourceError> {
        if !self.dir.is_dir() {
            return match &self.only {
                Some(only) if !only.is_empty() => Err(SourceError::NotStaged(only.join(", "))),
                _ => Ok(Vec::new()),
            };
        }
        let mut sources = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(SourceError::io(&self.dir))? {
            let path = entry.map_err(SourceError::io(&self.dir))?.path();
            let is_zip = path.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
            if !is_zip {
                continue;
            }
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let name: GenerationName =
                stem.parse()
                    .map_err(|source| SourceError::InvalidGenerationName {
                        file: stem.clone(),
                        source,
                    })?;
            if self
                .only
                .as_ref()
                .is_some_and(|only| !only.iter().any(|o| o == name.label()))
            {
                continue;
            }
            let kind = SourceKind::Generation(name);
            sources.push(DiscoveredSource {
                priority: kind.default_priority(),
                kind,
                location: SourceLocation::Zip(path),
            });
        }
        if let Some(only) = &self.only {
            let missing: Vec<_> = only
                .iter()
                .filter(|label| !sources.iter().any(|s| s.kind.label() == label.as_str()))
                .cloned()
                .collect();
            if !missing.is_empty() {
                return Err(SourceError::NotStaged(missing.join(", ")));
            }
        }
        sources.sort_by_key(|a| a.priority);
        Ok(sources)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pml_is_discovered_only_when_scripts_are_staged() {
        let dir = tempfile::tempdir().unwrap();
        let discovery = PmlDiscovery::new(dir.path().to_owned());
        assert_eq!(discovery.discover().unwrap(), Vec::new());
        fs::write(dir.path().join(".gitkeep"), "").unwrap();
        assert_eq!(discovery.discover().unwrap(), Vec::new());
        fs::write(dir.path().join("AMI_PRE_MODEL_INCR.sql"), "SELECT 1").unwrap();
        let found = discovery.discover().unwrap();
        assert_eq!(found[0].kind, SourceKind::Pml);
        assert_eq!(found[0].priority, Priority::HIGHEST);
    }

    #[test]
    fn zips_are_parsed_and_sorted_numerically() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "1134_FMC.zip",
            "113_DDL.zip",
            "999_GENERATE_ETL.zip",
            "notes.txt",
        ] {
            fs::write(dir.path().join(name), "").unwrap();
        }
        let labels: Vec<_> = ReleaseZipDiscovery::new(dir.path().to_owned())
            .discover()
            .unwrap()
            .iter()
            .map(|s| s.kind.label().to_owned())
            .collect();
        assert_eq!(labels, ["113_DDL", "999_GENERATE_ETL", "1134_FMC"]);
    }

    #[test]
    fn requested_generations_must_exist_and_never_prefix_match() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("1134_FMC.zip"), "").unwrap();
        let discovery =
            ReleaseZipDiscovery::new(dir.path().to_owned()).only(vec!["113_FMC".into()]);
        assert!(matches!(discovery.discover(), Err(SourceError::NotStaged(m)) if m == "113_FMC"));
    }

    #[test]
    fn invalid_zip_names_fail_loudly() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("release.zip"), "").unwrap();
        assert!(matches!(
            ReleaseZipDiscovery::new(dir.path().to_owned()).discover(),
            Err(SourceError::InvalidGenerationName { .. })
        ));
    }
}
