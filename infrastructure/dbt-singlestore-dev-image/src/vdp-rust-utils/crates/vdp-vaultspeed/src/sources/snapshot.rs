use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use vdp_vaultspeed_domain::{ArtifactKind, JobName, RelPath};

use crate::sources::SourceError;

/// How a job's file under `deployed/` is updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotMode {
    /// Append newly executed statements (DDL: the file reads as the job's change log).
    Append,
    /// Replace the file with the latest definition (ETL procedures, PML scripts). This
    /// stops the unbounded growth of the legacy files; history lives in the ledger.
    Overwrite,
}

/// Maintains `deployed/<KIND>/<dir>/<JOB>.sql`, a readable mirror of what ran.
/// It is documentation, not state: decisions are made from the ledger only.
#[derive(Debug, Clone)]
pub struct DeployedSnapshot {
    deployed_dir: PathBuf,
    modes: BTreeMap<ArtifactKind, SnapshotMode>,
}

impl DeployedSnapshot {
    pub fn new(deployed_dir: PathBuf, modes: BTreeMap<ArtifactKind, SnapshotMode>) -> Self {
        Self {
            deployed_dir,
            modes,
        }
    }

    pub fn mode(&self, artifact: ArtifactKind) -> SnapshotMode {
        self.modes
            .get(&artifact)
            .copied()
            .unwrap_or(SnapshotMode::Append)
    }

    /// Writes the statements executed for one job file and returns the path written.
    pub fn write(
        &self,
        artifact: ArtifactKind,
        rel_dir: &RelPath,
        job: &JobName,
        statements: &[&str],
    ) -> Result<PathBuf, SourceError> {
        let dir = self
            .deployed_dir
            .join(artifact.as_str())
            .join(rel_dir.as_str());
        fs::create_dir_all(&dir).map_err(SourceError::io(&dir))?;
        let path = dir.join(format!("{job}.sql"));
        let mut body = statements.join(";\n\n");
        body.push_str(";\n");

        match self.mode(artifact) {
            SnapshotMode::Overwrite => fs::write(&path, body).map_err(SourceError::io(&path))?,
            SnapshotMode::Append => {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(SourceError::io(&path))?;
                file.write_all(format!("\n{body}").as_bytes())
                    .map_err(SourceError::io(&path))?;
            }
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_replaces_and_append_accumulates() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = DeployedSnapshot::new(
            dir.path().to_owned(),
            BTreeMap::from([
                (ArtifactKind::Etl, SnapshotMode::Overwrite),
                (ArtifactKind::Ddl, SnapshotMode::Append),
            ]),
        );
        let job = JobName::from_file_stem("HUB_X_INCR");
        let dir_etl = RelPath::new("INCR/FOUNDATION_LAYER");
        snapshot
            .write(ArtifactKind::Etl, &dir_etl, &job, &["v1"])
            .unwrap();
        let path = snapshot
            .write(ArtifactKind::Etl, &dir_etl, &job, &["v2"])
            .unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "v2;\n");

        let ddl_job = JobName::from_file_stem("GENERATE_DDL_ALTER");
        snapshot
            .write(ArtifactKind::Ddl, &RelPath::new(""), &ddl_job, &["a1"])
            .unwrap();
        let path = snapshot
            .write(ArtifactKind::Ddl, &RelPath::new(""), &ddl_job, &["a2"])
            .unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "\na1;\n\na2;\n");
    }
}
