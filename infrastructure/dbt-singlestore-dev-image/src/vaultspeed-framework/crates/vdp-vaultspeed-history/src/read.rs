//! Read API: lifespans of objects and their statements.

use chrono::{DateTime, Utc};
use duckdb::{Row, params};

use crate::model::{ObjectRow, ObjectStatus, StatementRow};
use crate::{DuckDbHistory, DuckDbHistoryError};

const OBJECT_COLUMNS: &str = "object_id, environment, object_type, database, name, identifier, \
    first_identifier, origin, epoch_us(created_at), status, epoch_us(ended_at)";

const STATEMENT_COLUMNS: &str = "statement_id, object_id, identifier, operation, statement_kind, \
    column_name, renamed_to, raw_sql, formatted_sql, epoch_us(creation_time), epoch_us(run_time), \
    success, rows_affected, outcome, error_message, run_id, duration_ms";

fn micros(value: Option<i64>) -> Option<DateTime<Utc>> {
    value.and_then(DateTime::from_timestamp_micros)
}

fn object_row(row: &Row<'_>) -> duckdb::Result<ObjectRow> {
    Ok(ObjectRow {
        object_id: row.get(0)?,
        environment: row.get(1)?,
        object_type: row.get(2)?,
        database: row.get(3)?,
        name: row.get(4)?,
        identifier: row.get(5)?,
        first_identifier: row.get(6)?,
        origin: row.get(7)?,
        created_at: micros(row.get(8)?),
        status: row.get(9)?,
        ended_at: micros(row.get(10)?),
    })
}

fn statement_row(row: &Row<'_>) -> duckdb::Result<StatementRow> {
    Ok(StatementRow {
        statement_id: row.get(0)?,
        object_id: row.get(1)?,
        identifier: row.get(2)?,
        operation: row.get(3)?,
        statement_kind: row.get(4)?,
        column_name: row.get(5)?,
        renamed_to: row.get(6)?,
        raw_sql: row.get(7)?,
        formatted_sql: row.get(8)?,
        creation_time: micros(row.get(9)?).unwrap_or_default(),
        run_time: micros(row.get(10)?).unwrap_or_default(),
        success: row.get(11)?,
        rows_affected: row.get(12)?,
        outcome: row.get(13)?,
        error_message: row.get(14)?,
        run_id: row.get(15)?,
        duration_ms: row.get(16)?,
    })
}

impl DuckDbHistory {
    /// Every lifespan that ever carried `identifier` (`database.name`), oldest first. A name can be used by
    /// several objects over time (renamed away, recreated), and an object keeps matching its old names.
    ///
    /// # Errors
    ///
    /// [`DuckDbHistoryError::Poisoned`] when the connection lock was poisoned by a panic; [`DuckDbHistoryError::Db`] when the
    /// query fails.
    pub fn lifespans(
        &self,
        environment: &str,
        identifier: &str,
    ) -> Result<Vec<ObjectRow>, DuckDbHistoryError> {
        let conn = self.lock()?;
        let sql = format!(
            "SELECT {OBJECT_COLUMNS} FROM objects WHERE environment = ? AND object_id IN \
             (SELECT object_id FROM object_names WHERE identifier = ?) ORDER BY object_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![environment, identifier], object_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// All statements of the object that is (or was most recently) known under `identifier`, following its
    /// identity across renames, oldest first. Empty when the name was never seen.
    ///
    /// # Errors
    ///
    /// As [`DuckDbHistory::lifespans`].
    pub fn lifespan(
        &self,
        environment: &str,
        identifier: &str,
    ) -> Result<Vec<StatementRow>, DuckDbHistoryError> {
        let Some(object) = self.lifespans(environment, identifier)?.pop() else {
            return Ok(Vec::new());
        };
        let conn = self.lock()?;
        let sql = format!(
            "SELECT {STATEMENT_COLUMNS} FROM statements WHERE object_id = ? \
             ORDER BY run_time, statement_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![object.object_id], statement_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The objects of an environment, optionally only live or only dropped ones.
    ///
    /// # Errors
    ///
    /// As [`DuckDbHistory::lifespans`].
    pub fn objects(
        &self,
        environment: &str,
        status: Option<ObjectStatus>,
    ) -> Result<Vec<ObjectRow>, DuckDbHistoryError> {
        let conn = self.lock()?;
        let sql = format!(
            "SELECT {OBJECT_COLUMNS} FROM objects WHERE environment = ? \
             AND (? IS NULL OR status = ?) ORDER BY object_id"
        );
        let status = status.map(ObjectStatus::as_str);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![environment, status, status], object_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
