//! Pure domain model of VaultSpeed deployments (formerly the VaultSpeed Automation Framework, VSAF).
//!
//! Everything in this crate is plain data plus the rules that are intrinsic to that data
//! (parsing a generation name, deriving a job name from a file name, ...). Nothing here
//! performs IO; adapters in the other `vdp-vaultspeed*` crates produce and consume these types.

mod artifact;
mod change;
mod decision;
mod error;
mod evidence;
mod fingerprint;
mod generation;
mod group;
mod job;
mod layer;
mod ledger;
mod lifespan;
mod object;
mod path;
mod plan;
mod report;
mod run;
mod source;
mod statement;

pub use artifact::ArtifactKind;
pub use change::{ChangeEvent, ChangeOutcome, ChangeRecorder, HistoryError, NoHistory};
pub use decision::{Decision, ExecuteReason, SkipReason};
pub use error::DomainError;
pub use evidence::Evidence;
pub use fingerprint::Fingerprint;
pub use generation::{GenerationId, GenerationKind, GenerationName};
pub use group::{GroupId, JobInfo, StatementGroup};
pub use job::{JobName, OrderKey};
pub use layer::Layer;
pub use ledger::{LedgerEntry, LedgerOutcome};
pub use lifespan::{Lifespan, current_lifespan};
pub use object::{Facet, ObjectKey, ObjectRef, ObjectType};
pub use path::RelPath;
pub use plan::{PlanCounts, PlanPhase, PlannedGroup, SourcePlan};
pub use report::{
    DagReport, GroupReport, RunReport, RunTotals, SourceReport, StatementOutcome, StatementReport,
};
pub use run::{RunId, RunMode};
pub use source::{Priority, SourceKind};
pub use statement::{AlterOp, Statement, StatementKind};
pub use vdp_core::Environment;
