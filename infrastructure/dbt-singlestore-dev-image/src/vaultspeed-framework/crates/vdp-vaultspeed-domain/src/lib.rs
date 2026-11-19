//! Pure domain model of VaultSpeed deployments (formerly the VaultSpeed Automation Framework, VSAF).
//!
//! The innermost building block of the framework: the other `vdp-vaultspeed*` crates produce and consume these types, and
//! none of them is depended on from here. Nothing in this crate performs IO.
//!
//! # Responsibilities
//!
//! - Name things: generations ([`GenerationName`]), jobs ([`JobName`], [`OrderKey`]), relative paths ([`RelPath`]),
//!   database objects ([`ObjectRef`], [`ObjectKey`]) and statements ([`Statement`], [`StatementKind`]).
//! - Identify SQL by content ([`Fingerprint`]), so "has this already run?" does not depend on whitespace or comments.
//! - Describe a plan and its decisions ([`SourcePlan`], [`Decision`]) and what a run did ([`RunReport`]).
//! - Define what the ledger remembers ([`LedgerEntry`]) and how an object's lifespan follows from it
//!   ([`current_lifespan`]).
//! - Define the [`ChangeRecorder`] port the object history implements.
//!
//! # Example
//!
//! ```
//! use vdp_vaultspeed_domain::{Fingerprint, GenerationKind, GenerationName, JobName};
//!
//! let name: GenerationName = "1134_FMC".parse()?;
//! assert_eq!(name.id().get(), 1134);
//! assert_eq!(name.kind(), GenerationKind::Fmc);
//!
//! // Numbers and timestamps drop out, so the identity survives regeneration.
//! let job = JobName::from_file_stem("629_05_GENERATE_DDL_DV_competent_edw_dv_rdv");
//! assert_eq!(job.as_str(), "GENERATE_DDL_DV_COMPETENT_EDW_DV_RDV");
//!
//! assert_eq!(Fingerprint::of("select 1"), Fingerprint::of("select 1"));
//! # Ok::<(), vdp_vaultspeed_domain::DomainError>(())
//! ```

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
pub use vdp_core::{Environment, display_chain};
