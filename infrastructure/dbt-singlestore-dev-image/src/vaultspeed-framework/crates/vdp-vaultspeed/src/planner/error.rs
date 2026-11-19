use thiserror::Error;

use crate::planner::ProbeError;

#[derive(Debug, Error)]
pub enum PlanError {
    #[error(transparent)]
    Ledger(#[from] crate::ledger::LedgerError),

    #[error(transparent)]
    Probe(#[from] ProbeError),

    #[error("no idempotency policy decided group {0}; configure a fallback policy")]
    Undecided(String),
}
