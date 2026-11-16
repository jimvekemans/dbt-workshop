//! What exists in the database.

use std::fmt::Debug;
use std::sync::Arc;

use vdp_db::{DbError, SqlSession, SqlValue};

/// One column of one table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogColumn {
    pub table: String,
    pub column: String,
}

pub trait Catalog: Send + Sync + Debug {
    /// Tables and views of a database.
    fn tables(&self, database: &str) -> Result<Vec<String>, DbError>;
    /// Columns, of all tables or of the given ones, ordered by table and then by column (name or position).
    fn columns(
        &self,
        database: &str,
        tables: Option<&[String]>,
    ) -> Result<Vec<CatalogColumn>, DbError>;
}

/// [`Catalog`] over `information_schema`, with bound parameters (the Python utilities formatted names into SQL).
#[derive(Debug, Clone)]
pub struct SqlCatalog {
    session: Arc<dyn SqlSession>,
    by_position: bool,
}

impl SqlCatalog {
    pub fn new(session: Arc<dyn SqlSession>, column_order: &str) -> Self {
        Self {
            session,
            by_position: column_order == "ordinal",
        }
    }
}

impl Catalog for SqlCatalog {
    fn tables(&self, database: &str) -> Result<Vec<String>, DbError> {
        let rows = self.session.query(
            "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            &[SqlValue::from(database)],
        )?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                row.into_iter()
                    .next()
                    .and_then(|v| v.as_text().map(str::to_owned))
            })
            .collect())
    }

    fn columns(
        &self,
        database: &str,
        tables: Option<&[String]>,
    ) -> Result<Vec<CatalogColumn>, DbError> {
        let mut sql = String::from(
            "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ?",
        );
        let mut params = vec![SqlValue::from(database)];
        if let Some(tables) = tables {
            if tables.is_empty() {
                return Ok(Vec::new());
            }
            sql.push_str(" AND TABLE_NAME IN (");
            sql.push_str(&vec!["?"; tables.len()].join(", "));
            sql.push(')');
            params.extend(tables.iter().map(|t| SqlValue::from(t.as_str())));
        }
        sql.push_str(if self.by_position {
            " ORDER BY TABLE_NAME, ORDINAL_POSITION"
        } else {
            " ORDER BY TABLE_NAME, COLUMN_NAME"
        });
        let rows = self.session.query(&sql, &params)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let mut values = row.into_iter();
                let table = values.next()?.as_text()?.to_owned();
                let column = values.next()?.as_text()?.to_owned();
                Some(CatalogColumn { table, column })
            })
            .collect())
    }
}
