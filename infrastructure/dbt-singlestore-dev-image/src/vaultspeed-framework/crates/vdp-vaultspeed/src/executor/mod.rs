//! Execution of a [`vdp_vaultspeed_domain::SourcePlan`].
//!
//! [`PlanRunner`] walks the plan, executes groups through a [`StatementExecutor`],
//! turns "already exists" errors into evidence via an [`ErrorClassifier`], records every
//! statement in the ledger immediately (so an interrupted run resumes correctly) and
//! applies the configured [`FailurePolicy`].

mod classifier;
mod probe;
mod runner;
mod statement;

pub use classifier::{ErrorClassifier, ErrorVerdict, MySqlErrorClassifier, NoClassification};
pub use probe::InformationSchemaProbe;
pub use runner::{
    ExecError, FailurePolicy, Halt, HaltScope, PlanRunner, PlanRunnerParts, RunContext, SourceRun,
};
pub use statement::{DryRunExecutor, SessionExecutor, StatementExecutor};
