use std::fmt;

use serde::{Deserialize, Serialize};

/// The nature of a SQL artifact; it decides how a file is split, rewritten and ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ArtifactKind {
    /// Hand-written pre-model logic from `staging/pre_model_input`.
    Pml,
    /// `VaultSpeed` data definition (tables, views).
    Ddl,
    /// `VaultSpeed` load procedures and delta-firewall views.
    Etl,
}

impl ArtifactKind {
    /// The short name of the kind (`PML`, `DDL`, `ETL`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pml => "PML",
            Self::Ddl => "DDL",
            Self::Etl => "ETL",
        }
    }
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
