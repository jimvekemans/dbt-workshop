use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Fingerprint, ObjectRef, RunId, StatementKind};

/// What happened to one statement in one run, as far as the object history is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeOutcome {
    /// Executed successfully.
    Applied,
    /// Not executed, or failed harmlessly, because the desired state already held (catalog evidence).
    Satisfied,
    /// Executed and failed.
    Failed,
    /// Part of a group that was skipped (already applied, excluded, halted, ...); nothing ran.
    Skipped,
    /// Part of a group the planner refused; nothing ran.
    Refused,
    /// Part of an executing group, but not reached because an earlier statement of the group failed.
    NotRun,
}

impl ChangeOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Satisfied => "satisfied",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Refused => "refused",
            Self::NotRun => "not_run",
        }
    }

    /// Whether the statement's effect is in the database after this event (it ran, or it already held).
    /// Only such events change an object's lifespan.
    pub const fn took_effect(self) -> bool {
        matches!(self, Self::Applied | Self::Satisfied)
    }

    /// Whether the database executed the statement (successfully or not).
    pub const fn executed(self) -> bool {
        matches!(self, Self::Applied | Self::Failed)
    }
}

impl fmt::Display for ChangeOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One statement of one run with everything needed to reconstruct an object's lifespan: what it targets
/// (database-qualified), its SQL, when it ran, whether it succeeded and how many rows it touched.
///
/// Produced by the plan runner for every statement of every group (executed or not); consumed by a
/// change recorder such as the local DuckDB object history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub run_id: RunId,
    /// The environment name as configured (`LDV`, `dev`, ...).
    pub environment: String,
    /// `PML`, `1134_FMC`, ...
    pub source: String,
    pub job: String,
    /// `<artifact>/<path>` as in the ledger.
    pub rel_path: String,
    pub group_id: String,
    pub phase: String,
    /// Position of the statement within its group (0-based).
    pub statement_index: u32,
    pub kind: StatementKind,
    /// The object the statement changes, qualified with its database (unqualified names are resolved to the
    /// database in effect: the environment's default or the last `USE`).
    pub target: Option<ObjectRef>,
    /// Text exactly as delivered.
    pub raw_sql: String,
    /// Text that was (or would have been) executed, after preprocessing and rewrite rules.
    pub effective_sql: String,
    pub raw_fp: Fingerprint,
    pub effective_fp: Fingerprint,
    pub applied_rules: Vec<String>,
    /// The planner's verdict for the group, as displayed (`execute: new`, `skip: already applied (ledger #12)`).
    pub decision: String,
    pub outcome: ChangeOutcome,
    /// Rows the database reported as affected, when it reported a count.
    pub rows_affected: Option<i64>,
    pub error_code: Option<u16>,
    pub error_message: Option<String>,
    /// Ledger sequence of the entry recorded for this statement, if one was recorded.
    pub ledger_sequence: Option<u64>,
    /// When execution of the statement started (for statements that did not run: when the decision was made).
    pub run_time: DateTime<Utc>,
    /// Wall-clock duration of the execution; `None` when nothing ran.
    pub duration_ms: Option<u64>,
    pub git_sha: Option<String>,
}

impl ChangeEvent {
    /// The rename target, for `ALTER TABLE ... RENAME TO ...`.
    pub fn renamed_to(&self) -> Option<&ObjectRef> {
        match &self.kind {
            StatementKind::Alter {
                op: crate::AlterOp::Rename { to },
            } => Some(to),
            _ => None,
        }
    }

    /// The column an `ALTER TABLE ... ADD/MODIFY/CHANGE/DROP COLUMN` changes.
    pub fn column(&self) -> Option<&str> {
        match &self.kind {
            StatementKind::Alter {
                op:
                    crate::AlterOp::AddColumn { column, .. }
                    | crate::AlterOp::ModifyColumn { column, .. }
                    | crate::AlterOp::ChangeColumn { column }
                    | crate::AlterOp::DropColumn { column },
            } => Some(column),
            _ => None,
        }
    }
}

/// Why a change recorder could not record an event.
#[derive(Debug, thiserror::Error)]
#[error("object history: {0}")]
pub struct HistoryError(pub String);

impl HistoryError {
    pub fn new(message: impl fmt::Display) -> Self {
        Self(message.to_string())
    }
}

/// Receives every statement event of a run, in execution order (the port of the object history).
///
/// The history is an audit record: it never decides what runs (the ledger does), so callers log a failing
/// recorder and carry on.
pub trait ChangeRecorder: Send + Sync + fmt::Debug {
    fn record(&self, event: &ChangeEvent) -> Result<(), HistoryError>;
}

/// Records nothing (history disabled, preview runs).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHistory;

impl ChangeRecorder for NoHistory {
    fn record(&self, _: &ChangeEvent) -> Result<(), HistoryError> {
        Ok(())
    }
}
