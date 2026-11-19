//! The orchestration of one run.
//!
//! [`Engine`] depends only on abstractions (discoveries, handlers, planner, runner,
//! lifecycles, DAG stage); `vsaf-cli` decides which implementations to plug in. Sources
//! are processed one at a time in priority order (PML first), and each source is planned
//! *after* the previous one has executed, so its plan sees everything that ran before it.
//!
//! A run has three phases that can also be driven one by one: [`Engine::discover_sources`],
//! [`Engine::prepare_source`] (load and plan, producing a [`PreparedSource`]) and
//! [`Engine::execute_source`] (consumes the [`PreparedSource`]).

mod dag_stage;
mod error;
mod failure;
mod milestone;
mod orchestrator;
mod prepared;

pub use dag_stage::{DagStage, DagStageParts};
pub use error::EngineError;
pub use failure::FailurePolicies;
pub use orchestrator::{Engine, EngineParts};
pub use prepared::{ExecutedSource, PreparedSource};
