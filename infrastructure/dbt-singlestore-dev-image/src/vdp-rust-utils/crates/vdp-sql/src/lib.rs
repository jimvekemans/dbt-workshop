//! `vdp sql run` (`call_sql.py`) and `vdp sql export` (`download_from_singlestore.py`).
//!
//! The Python export formatted the table name into `SELECT * FROM {name}`; here names are validated and
//! quoted. `run` no longer prompts: results print, and `--csv-dir` saves them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use thiserror::Error;
use vdp_db::{DbError, ResultSet, SqlSession, write_csv};

#[derive(Debug, Error)]
pub enum SqlError {
    #[error("`{0}` is not a table name (database.table, letters, digits, _ and $)")]
    InvalidName(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Db(#[from] DbError),
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SqlError + '_ {
    move |source| SqlError::Io {
        path: path.to_owned(),
        source,
    }
}

/// `db.table` or `table`, quoted for SingleStore: `` `db`.`table` ``.
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

/// Runs a SQL file (one or more statements); returns the last result set with columns.
pub fn run_file(session: &dyn SqlSession, file: &Path) -> Result<ResultSet, SqlError> {
    let sql = std::fs::read_to_string(file).map_err(io(file))?;
    Ok(session.query_table(&sql, &[])?)
}

/// `SELECT *` of a table.
pub fn export_table(session: &dyn SqlSession, name: &str) -> Result<ResultSet, SqlError> {
    let sql = format!("SELECT * FROM {}", quote_table(name)?);
    Ok(session.query_table(&sql, &[])?)
}

/// Writes a result set as CSV, creating the directory.
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
    let widths: Vec<usize> = result
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            rows.iter()
                .map(|r| r.get(i).map_or(0, |v| v.chars().count()))
                .chain([c.chars().count()])
                .max()
                .unwrap_or(0)
                .min(60)
        })
        .collect();
    let line = |cells: &[String]| -> String {
        cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| {
                let cell: String = cell.chars().take(*width).collect();
                format!("{cell:<width$}")
            })
            .collect::<Vec<_>>()
            .join(" | ")
            .trim_end()
            .to_owned()
    };
    let mut out = String::new();
    let _ = writeln!(out, "{}", line(&result.columns));
    let _ = writeln!(
        out,
        "{}",
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("-+-")
    );
    for row in &rows {
        let _ = writeln!(out, "{}", line(row));
    }
    let shown = rows.len();
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
