use std::fmt::{self, Debug};

use thiserror::Error;

/// A parameter or column value.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Int(i64),
    UInt(u64),
    Float(f64),
    Text(String),
}

impl SqlValue {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::UInt(v) => Some(*v),
            Self::Int(v) => u64::try_from(*v).ok(),
            Self::Text(text) => text.parse().ok(),
            _ => None,
        }
    }
}

impl From<&str> for SqlValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for SqlValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Option<String>> for SqlValue {
    fn from(value: Option<String>) -> Self {
        value.map_or(Self::Null, Self::Text)
    }
}

impl From<u64> for SqlValue {
    fn from(value: u64) -> Self {
        Self::UInt(value)
    }
}

impl fmt::Display for SqlValue {
    /// The value as a CSV cell or console text; `NULL` is empty.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => Ok(()),
            Self::Int(v) => write!(f, "{v}"),
            Self::UInt(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Text(v) => f.write_str(v),
        }
    }
}

/// One result row, columns in select order.
pub type Row = Vec<SqlValue>;

/// Rows plus their column names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

/// A database error, with the server error code when the server produced it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub struct DbError {
    pub code: Option<u16>,
    pub message: String,
}

impl DbError {
    pub fn client(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "({code}) {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// A single database session. Statements run in order on one connection, so session
/// state (`SET ...`) carries over between statements of a script.
pub trait SqlSession: Send + Sync + Debug {
    /// Executes a statement without parameters, discarding any result.
    fn execute(&self, sql: &str) -> Result<(), DbError>;

    /// Executes a parameterised statement (`?` placeholders), discarding any result.
    fn execute_with(&self, sql: &str, params: &[SqlValue]) -> Result<(), DbError>;

    /// Runs a parameterised query and returns all rows.
    fn query(&self, sql: &str, params: &[SqlValue]) -> Result<Vec<Row>, DbError>;

    /// Like [`SqlSession::query`], with column names. Without parameters the text may hold several
    /// statements; the last result set with columns is returned.
    fn query_table(&self, sql: &str, params: &[SqlValue]) -> Result<ResultSet, DbError> {
        Ok(ResultSet {
            columns: Vec::new(),
            rows: self.query(sql, params)?,
        })
    }
}
