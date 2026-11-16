use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a statement is considered already in effect without (re-)executing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Evidence {
    /// `information_schema` shows the table already exists.
    TablePresent { table: String },
    /// `information_schema` shows the column already exists.
    ColumnPresent {
        table: String,
        column: String,
        data_type: String,
    },
    /// `information_schema` shows the column is already gone.
    ColumnAbsent { table: String, column: String },
    /// The rename source is gone and the target exists.
    AlreadyRenamed { from: String, to: String },
    /// The database answered with an error that proves the desired state.
    ErrorCode { code: u16, message: String },
}

impl fmt::Display for Evidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TablePresent { table } => write!(f, "table {table} already exists"),
            Self::ColumnPresent {
                table,
                column,
                data_type,
            } => write!(f, "column {table}.{column} already exists ({data_type})"),
            Self::ColumnAbsent { table, column } => {
                write!(f, "column {table}.{column} is already absent")
            }
            Self::AlreadyRenamed { from, to } => write!(f, "{from} was already renamed to {to}"),
            Self::ErrorCode { code, message } => {
                write!(f, "database error {code} proves desired state: {message}")
            }
        }
    }
}
