use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Evidence, Fingerprint};

/// Why a group will run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum ExecuteReason {
    /// Never seen before in this environment.
    New,
    /// A replaceable object whose definition differs from the last applied version.
    Changed { previous: Option<Fingerprint> },
    /// Staged PML always runs; staging it is the trigger.
    AlwaysRun,
    /// Ran before, but the state it produced was replaced since (ledger `since`):
    /// the column was dropped, the table renamed away or rebuilt, ...
    Stale { since: u64 },
    /// The ledger says the object exists, but the catalog does not know it: it was renamed or dropped
    /// outside the framework.
    Vanished { detail: String },
}

/// Why a group will not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SkipReason {
    /// The ledger records this exact change as applied.
    AlreadyApplied { sequence: u64 },
    /// The catalog proves the change is already in effect.
    Satisfied { evidence: Evidence },
    /// A configured exclusion rule matched.
    Excluded { rule: String },
    /// An earlier group that creates the same object failed in this run.
    UpstreamFailed { object: String },
    /// The run was halted by a failure policy before this group was reached.
    Halted,
    /// An earlier group of the same plan executes the very same statements.
    DuplicateInPlan { group: String },
}

/// The planner's verdict for one statement group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Execute(ExecuteReason),
    Skip(SkipReason),
    /// Neither running nor skipping is safe; a person must decide. Reported as a failure.
    Refuse {
        reason: String,
    },
}

impl Decision {
    pub const fn is_execute(&self) -> bool {
        matches!(self, Self::Execute(_))
    }

    pub const fn is_refused(&self) -> bool {
        matches!(self, Self::Refuse { .. })
    }
}

impl fmt::Display for Decision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execute(ExecuteReason::New) => f.write_str("execute: new"),
            Self::Execute(ExecuteReason::Changed { previous: Some(fp) }) => {
                write!(f, "execute: changed (was {})", fp.short())
            }
            Self::Execute(ExecuteReason::Changed { previous: None }) => {
                f.write_str("execute: changed (object was dropped)")
            }
            Self::Execute(ExecuteReason::AlwaysRun) => f.write_str("execute: always (staged PML)"),
            Self::Execute(ExecuteReason::Stale { since }) => {
                write!(f, "execute: state replaced since it ran (ledger #{since})")
            }
            Self::Execute(ExecuteReason::Vanished { detail }) => {
                write!(f, "execute: object vanished from the catalog ({detail})")
            }
            Self::Skip(SkipReason::AlreadyApplied { sequence }) => {
                write!(f, "skip: already applied (ledger #{sequence})")
            }
            Self::Skip(SkipReason::Satisfied { evidence }) => write!(f, "skip: {evidence}"),
            Self::Skip(SkipReason::Excluded { rule }) => write!(f, "skip: excluded by {rule}"),
            Self::Skip(SkipReason::UpstreamFailed { object }) => {
                write!(f, "skip: creation of {object} failed earlier in this run")
            }
            Self::Skip(SkipReason::Halted) => f.write_str("skip: run halted"),
            Self::Skip(SkipReason::DuplicateInPlan { group }) => {
                write!(f, "skip: same statements already executed by {group}")
            }
            Self::Refuse { reason } => write!(f, "refused: {reason}"),
        }
    }
}
