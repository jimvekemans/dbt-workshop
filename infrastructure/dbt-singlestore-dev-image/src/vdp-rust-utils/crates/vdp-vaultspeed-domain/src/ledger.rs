use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Fingerprint, RunId};

/// Result of a statement as remembered by the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LedgerOutcome {
    /// Executed successfully.
    Applied,
    /// Not executed (or failed harmlessly) because the desired state already held.
    Satisfied,
    /// Seeded from the legacy `vaultspeed/deployed` files.
    Imported,
    /// Executed and failed; kept for audit, never counts as "has run".
    Failed,
}

impl LedgerOutcome {
    pub const fn counts_as_applied(self) -> bool {
        !matches!(self, Self::Failed)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Satisfied => "satisfied",
            Self::Imported => "imported",
            Self::Failed => "failed",
        }
    }
}

impl std::str::FromStr for LedgerOutcome {
    type Err = crate::DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "applied" => Ok(Self::Applied),
            "satisfied" => Ok(Self::Satisfied),
            "imported" => Ok(Self::Imported),
            "failed" => Ok(Self::Failed),
            other => Err(crate::DomainError::UnknownVariant {
                what: "ledger outcome",
                value: other.to_owned(),
            }),
        }
    }
}

/// One remembered statement execution in one environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// Monotonic position assigned by the ledger; later entries win.
    pub sequence: u64,
    pub run_id: RunId,
    pub recorded_at: DateTime<Utc>,
    /// `PML`, `1134_FMC`, `legacy:deployed`, ...
    pub source: String,
    pub job: String,
    pub rel_path: String,
    /// `ObjectKey` text, when the statement targets a known object.
    pub object_key: Option<String>,
    /// `StatementKind::label()`.
    pub statement_kind: String,
    /// Absent for entries imported from legacy state, which only kept executed SQL.
    pub raw_fp: Option<Fingerprint>,
    pub effective_fp: Fingerprint,
    pub outcome: LedgerOutcome,
    pub detail: Option<String>,
    pub git_sha: Option<String>,
}

impl LedgerEntry {
    /// Whether this entry ended the object's existence under its name (DROP or RENAME).
    pub fn ends_definition(&self) -> bool {
        self.statement_kind.starts_with("DROP") || self.statement_kind == "ALTER TABLE RENAME"
    }

    /// Entries from real runs are totally ordered. Imported legacy entries are not: the
    /// Python framework kept one file per job, so their relative order across files is
    /// unknown and must never be used to conclude that one change superseded another.
    pub const fn is_ordered(&self) -> bool {
        !matches!(
            self.outcome,
            LedgerOutcome::Imported | LedgerOutcome::Failed
        )
    }
}
