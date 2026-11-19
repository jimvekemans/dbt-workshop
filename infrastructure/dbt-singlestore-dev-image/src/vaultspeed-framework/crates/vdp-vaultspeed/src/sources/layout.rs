use std::path::{Path, PathBuf};

use vdp_vaultspeed_domain::ArtifactKind;

/// The `vaultspeed/` directory layout shared with the Python framework.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagingLayout {
    root: PathBuf,
}

impl StagingLayout {
    /// Creates the layout below `root`, the `vaultspeed/` directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `staging/release`: generation zips waiting to be deployed.
    pub fn release_dir(&self) -> PathBuf {
        self.root.join("staging").join("release")
    }

    /// `staging/pre_model_input`: PML scripts waiting to be deployed.
    pub fn pml_dir(&self) -> PathBuf {
        self.root.join("staging").join("pre_model_input")
    }

    /// `staging/manual_changes/<KIND>`: rewrite rules.
    pub fn rules_dir(&self, artifact: ArtifactKind) -> PathBuf {
        self.root
            .join("staging")
            .join("manual_changes")
            .join(artifact.as_str())
    }

    /// `deployed`: human-readable mirror of executed SQL.
    pub fn deployed_dir(&self) -> PathBuf {
        self.root.join("deployed")
    }

    /// `deployed/archive`: processed generation zips.
    pub fn archive_dir(&self) -> PathBuf {
        self.deployed_dir().join("archive")
    }

    /// `airflow/dags` and `airflow/meta`: rendered DAGs committed to git.
    pub fn airflow_dags_dir(&self) -> PathBuf {
        self.root.join("airflow").join("dags")
    }

    /// `airflow/meta`: the metadata files the rendered DAGs read.
    pub fn airflow_meta_dir(&self) -> PathBuf {
        self.root.join("airflow").join("meta")
    }

    /// `logs/<source>`: per-source error reports.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// `ledger`: JSONL audit ledgers.
    pub fn ledger_dir(&self) -> PathBuf {
        self.root.join("ledger")
    }
}
