//! Row types returned by the read API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Whether an object lifespan is still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectStatus {
    Live,
    Dropped,
}

impl ObjectStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Dropped => "dropped",
        }
    }
}

/// One object lifespan (a row of `objects`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRow {
    pub object_id: i64,
    pub environment: String,
    pub object_type: String,
    pub database: Option<String>,
    /// Current name (the last one, when dropped).
    pub name: String,
    pub identifier: String,
    pub first_identifier: String,
    /// `created` or `untracked`.
    pub origin: String,
    pub created_at: Option<DateTime<Utc>>,
    /// `live` or `dropped`.
    pub status: String,
    pub ended_at: Option<DateTime<Utc>>,
}

/// One recorded statement (the most useful columns of `statements`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementRow {
    pub statement_id: i64,
    pub object_id: Option<i64>,
    pub identifier: Option<String>,
    pub operation: String,
    pub statement_kind: String,
    pub column_name: Option<String>,
    pub renamed_to: Option<String>,
    pub raw_sql: String,
    pub formatted_sql: String,
    pub creation_time: DateTime<Utc>,
    pub run_time: DateTime<Utc>,
    /// `None` when nothing ran.
    pub success: Option<bool>,
    pub rows_affected: Option<i64>,
    pub outcome: String,
    pub error_message: Option<String>,
    pub run_id: Option<String>,
    /// Wall-clock duration of the execution; `None` when nothing ran.
    pub duration_ms: Option<i64>,
}
