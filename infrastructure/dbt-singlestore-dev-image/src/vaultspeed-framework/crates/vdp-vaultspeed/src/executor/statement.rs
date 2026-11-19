use std::fmt::Debug;
use std::sync::Arc;

use vdp_db::{DbError, SqlSession};

/// Executes one statement.
pub trait StatementExecutor: Send + Sync + Debug {
    /// Runs the statement and returns the number of rows the database reported as affected, when it
    /// reported one.
    fn execute(&self, sql: &str) -> Result<Option<i64>, DbError>;
}

/// Executes against a live database session.
#[derive(Debug, Clone)]
pub struct SessionExecutor {
    session: Arc<dyn SqlSession>,
}

impl SessionExecutor {
    pub fn new(session: Arc<dyn SqlSession>) -> Self {
        Self { session }
    }
}

impl StatementExecutor for SessionExecutor {
    fn execute(&self, sql: &str) -> Result<Option<i64>, DbError> {
        self.session.execute_counted(sql)
    }
}

/// Executes nothing and reports success; used by preview runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct DryRunExecutor;

impl StatementExecutor for DryRunExecutor {
    fn execute(&self, sql: &str) -> Result<Option<i64>, DbError> {
        tracing::debug!(sql, "preview: statement not executed");
        Ok(None)
    }
}
