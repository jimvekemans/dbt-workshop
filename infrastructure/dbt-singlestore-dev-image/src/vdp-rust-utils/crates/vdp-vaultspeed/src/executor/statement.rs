use std::fmt::Debug;
use std::sync::Arc;

use vdp_db::{DbError, SqlSession};

/// Executes one statement.
pub trait StatementExecutor: Send + Sync + Debug {
    fn execute(&self, sql: &str) -> Result<(), DbError>;
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
    fn execute(&self, sql: &str) -> Result<(), DbError> {
        self.session.execute(sql)
    }
}

/// Executes nothing and reports success; used by preview runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct DryRunExecutor;

impl StatementExecutor for DryRunExecutor {
    fn execute(&self, sql: &str) -> Result<(), DbError> {
        tracing::debug!(sql, "preview: statement not executed");
        Ok(())
    }
}
