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
    /// Whether the outcome counts as "has run" (everything but a failure).
    pub const fn counts_as_applied(self) -> bool {
        !matches!(self, Self::Failed)
    }

    /// The lower-case name of the outcome, as stored.
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
    /// For a rename: the definition `ObjectKey` text of the new name. A rename is one event with two
    /// subjects, so ledgers index it under both names; without this an object that comes back under its
    /// old name could not be told apart from one that never left.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed_to: Option<String>,
}

/// `StatementKind::label()` of `ALTER TABLE ... RENAME TO ...`.
const RENAME_LABEL: &str = "ALTER TABLE RENAME";

impl LedgerEntry {
    /// Whether this entry is a table rename.
    pub fn is_rename(&self) -> bool {
        self.statement_kind == RENAME_LABEL
    }

    /// Whether this entry ended the existence of the object living under definition key `key`: a DROP, or a
    /// rename *away* from `key` (a rename recorded under its target does not end the target).
    pub fn ends_definition_of(&self, key: &str) -> bool {
        self.statement_kind.starts_with("DROP")
            || (self.is_rename() && self.object_key.as_deref() == Some(key))
    }

    /// Whether this entry started an object living under definition key `key`: a CREATE, or a rename *into*
    /// `key`.
    pub fn begins_definition_of(&self, key: &str) -> bool {
        self.statement_kind.starts_with("CREATE")
            || (self.is_rename() && self.renamed_to.as_deref() == Some(key))
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
