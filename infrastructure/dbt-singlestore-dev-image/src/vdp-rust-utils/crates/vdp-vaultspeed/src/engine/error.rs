use thiserror::Error;

/// Errors that abort a run. Everything else is recorded in the report.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("source discovery failed: {0}")]
    Discovery(#[from] crate::sources::SourceError),

    #[error("planning failed: {0}")]
    Plan(#[from] crate::planner::PlanError),

    #[error(transparent)]
    Execution(#[from] crate::executor::ExecError),
}
