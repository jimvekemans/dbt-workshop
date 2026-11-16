//! The orchestration of one run.
//!
//! [`Engine`] depends only on abstractions (discoveries, handlers, planner, runner,
//! lifecycles, DAG stage); `vsaf-cli` decides which implementations to plug in. Sources
//! are processed one at a time in priority order (PML first), and each source is planned
//! *after* the previous one has executed, so its plan sees everything that ran before it.

mod dag_stage;
mod error;
mod failure;
mod orchestrator;

pub use dag_stage::DagStage;
pub use error::EngineError;
pub use failure::FailurePolicies;
pub use orchestrator::{Engine, EngineParts};
