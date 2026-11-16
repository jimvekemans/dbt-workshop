use std::fmt;

use serde::{Deserialize, Serialize};

/// Identifier of one framework invocation; links ledger entries to reports.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(String);

impl RunId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether side effects (database, Airflow, filesystem state) are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    Apply,
    /// Plans and renders everything, mutates nothing (not even staging files).
    Preview,
}

impl RunMode {
    pub const fn is_preview(self) -> bool {
        matches!(self, Self::Preview)
    }
}
