//! `vdp sql run` and `vdp sql export`.
//!
//! A small domain module over the [`vdp_db::SqlSession`] port. The Python export formatted the table name into
//! `SELECT * FROM {name}`; here names are validated and quoted. `run` no longer prompts: results print, and `--csv-dir`
//! saves them.
//!
//! # Responsibilities
//!
//! - Validate and quote a table name ([`quote_table`]).
//! - Run a SQL file, one or more statements, and return the last result set ([`run_file`], [`read_sql_file`]).
//! - Export a table ([`export_table`]).
//! - Save a result set as CSV ([`save_csv`]).
//!
//! # Example
//!
//! ```
//! use vdp_sql::quote_table;
//!
//! assert_eq!(quote_table("edw_dv_rdv.hub_customer")?, "`edw_dv_rdv`.`hub_customer`");
//! // Anything that is not a plain name is refused instead of formatted into SQL.
//! assert!(quote_table("hub; DROP TABLE x").is_err());
//! # Ok::<(), vdp_sql::SqlError>(())
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use thiserror::Error;
use vdp_core::present::Table;
use vdp_db::{DbError, ResultSet, SqlSession, write_csv};

/// Why a SQL command failed.
#[derive(Debug, Error)]
pub enum SqlError {
    #[error("`{0}` is not a table name (database.table, letters, digits, _ and $)")]
    InvalidName(String),
    #[error("{path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Db(#[from] DbError),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SqlError + '_ {
    move |source| SqlError::Io {
        path: path.to_owned(),
        source,
    }
}

/// `db.table` or `table`, quoted for SingleStore: `` `db`.`table` ``.
///
/// # Errors
///
/// [`SqlError::InvalidName`] when `name` is not `table` or `db.table` made of letters, digits, `_` and `$`.
pub fn quote_table(name: &str) -> Result<String, SqlError> {
    let parts: Vec<&str> = name.split('.').collect();
    let valid = (1..=2).contains(&parts.len())
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        });
    if !valid {
        return Err(SqlError::InvalidName(name.to_owned()));
    }
    Ok(parts
        .iter()
        .map(|p| format!("`{p}`"))
        .collect::<Vec<_>>()
        .join("."))
}

/// Reads a SQL file from disk without connecting to the database.
///
/// # Errors
///
/// [`SqlError::Io`] when the file cannot be read.
pub fn read_sql_file(file: &Path) -> Result<String, SqlError> {
    std::fs::read_to_string(file).map_err(io(file))
}

/// Runs a SQL file (one or more statements); returns the last result set with columns.
///
/// # Errors
///
/// [`SqlError::Io`] when the file cannot be read; [`SqlError::Db`] when a statement fails.
pub fn run_file(session: &dyn SqlSession, file: &Path) -> Result<ResultSet, SqlError> {
    let sql = read_sql_file(file)?;
    Ok(session.query_table(&sql, &[])?)
}

/// `SELECT *` of a table.
///
/// # Errors
///
/// [`SqlError::InvalidName`] when `name` is not a valid table name; [`SqlError::Db`] when the query fails.
pub fn export_table(session: &dyn SqlSession, name: &str) -> Result<ResultSet, SqlError> {
    let sql = format!("SELECT * FROM {}", quote_table(name)?);
    Ok(session.query_table(&sql, &[])?)
}

/// Writes a result set as CSV, creating the directory.
///
/// # Errors
///
/// [`SqlError::Io`] when the directory or file cannot be created or written.
pub fn save_csv(result: &ResultSet, path: &Path) -> Result<(), SqlError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    let mut file = std::fs::File::create(path).map_err(io(path))?;
    write_csv(result, &mut file).map_err(io(path))
}

/// A plain-text table of at most `limit` rows.
pub fn render(result: &ResultSet, limit: usize) -> String {
    if result.columns.is_empty() {
        return "(no result set)\n".into();
    }
    let rows: Vec<Vec<String>> = result
        .rows
        .iter()
        .take(limit)
        .map(|row| row.iter().map(ToString::to_string).collect())
        .collect();
    let shown = rows.len();
    let mut out = Table::new(&result.columns)
        .rows(rows)
        .separator(" | ")
        .divider("-+-")
        .max_col_width(60)
        .render();
    let total = result.rows.len();
    if total > shown {
        let _ = writeln!(out, "... {} more rows", total - shown);
    }
    let _ = writeln!(out, "({total} rows)");
    out
}

#[cfg(test)]
mod tests {
    use vdp_db::SqlValue;

    use super::*;

    #[test]
    fn quotes_only_valid_names() {
        assert_eq!(quote_table("edw_inc.t1").unwrap(), "`edw_inc`.`t1`");
        assert_eq!(quote_table("t$1").unwrap(), "`t$1`");
        for bad in ["", "a.b.c", "a; drop table x", "a`b", ".t", "db."] {
            assert!(quote_table(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn renders_a_table() {
        let result = ResultSet {
            columns: vec!["id".into(), "name".into()],
            rows: vec![
                vec![SqlValue::Int(1), SqlValue::Text("a".into())],
                vec![SqlValue::Int(22), SqlValue::Null],
            ],
        };
        assert_eq!(
            render(&result, 1),
            "id | name\n---+-----\n1  | a\n... 1 more rows\n(2 rows)\n"
        );
    }
}
