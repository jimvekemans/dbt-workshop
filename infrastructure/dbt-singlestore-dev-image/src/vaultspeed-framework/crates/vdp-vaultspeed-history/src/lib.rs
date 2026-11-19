//! Local `DuckDB` object history of `VaultSpeed` deployments.
//!
//! Every statement the framework handles is stored in `statements` and linked to the database object it
//! changes, so the whole lifespan of an object (create, alter, data changes, rename, drop) can be read back,
//! also across renames. The tables are meant to be queried directly with the `duckdb` CLI; the read API of
//! [`DuckDbHistory`] covers the common questions.

mod model;
mod read;
mod record;

use std::fmt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use duckdb::Connection;
use vdp_vaultspeed_domain::{ChangeEvent, ChangeRecorder, HistoryError};

pub use model::{ObjectRow, ObjectStatus, StatementRow};

/// Version of the table layout, stored in `meta` so later releases can migrate existing files.
const SCHEMA_VERSION: i64 = 1;
const SCHEMA_SQL: &str = include_str!("schema.sql");

/// Failures of the history database itself.
#[derive(Debug, thiserror::Error)]
pub enum DuckDbHistoryError {
    #[error("duckdb: {0}")]
    Db(#[from] duckdb::Error),
    #[error("cannot create {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("history file has schema version {found}, this build understands up to {supported}")]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("history connection lock was poisoned")]
    Poisoned,
}

impl From<DuckDbHistoryError> for HistoryError {
    fn from(error: DuckDbHistoryError) -> Self {
        Self::new(error)
    }
}

/// The `DuckDB`-backed [`ChangeRecorder`].
///
/// One connection behind a mutex: events arrive one at a time in execution order, and every event is
/// recorded in its own transaction so a crash never leaves a statement without its object bookkeeping.
pub struct DuckDbHistory {
    conn: Mutex<Connection>,
}

impl fmt::Debug for DuckDbHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DuckDbHistory").finish_non_exhaustive()
    }
}

impl DuckDbHistory {
    /// Opens (creating it and its parent directories when needed) the history file and makes sure the
    /// schema exists. Safe to call repeatedly on the same file.
    pub fn open(path: &Path) -> Result<Self, DuckDbHistoryError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| DuckDbHistoryError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        Self::init(Connection::open(path)?)
    }

    /// A throw-away history in memory, for tests.
    pub fn in_memory() -> Result<Self, DuckDbHistoryError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, DuckDbHistoryError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key VARCHAR PRIMARY KEY, value VARCHAR NOT NULL);",
        )?;
        let found: Option<i64> = conn
            .query_row(
                "SELECT CAST(value AS BIGINT) FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .ok();
        match found {
            Some(found) if found > SCHEMA_VERSION => {
                return Err(DuckDbHistoryError::SchemaTooNew {
                    found,
                    supported: SCHEMA_VERSION,
                });
            }
            // Older versions would be migrated here, step by step, before the idempotent schema pass.
            Some(_) | None => {}
        }
        conn.execute_batch(SCHEMA_SQL)?;
        conn.execute(
            "INSERT INTO meta VALUES ('schema_version', ?) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [SCHEMA_VERSION.to_string()],
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, DuckDbHistoryError> {
        self.conn.lock().map_err(|_| DuckDbHistoryError::Poisoned)
    }
}

impl ChangeRecorder for DuckDbHistory {
    fn record(&self, event: &ChangeEvent) -> Result<(), HistoryError> {
        let mut conn = self.lock()?;
        record::record_event(&mut conn, event).map_err(HistoryError::from)
    }
}

#[cfg(test)]
mod tests;
