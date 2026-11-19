//! Planning: which statement groups run, and in which order.
//!
//! [`Planner`] asks an ordered chain of [`IdempotencyPolicy`] implementations for a
//! verdict on every group (the first policy with an opinion wins) and orders groups with
//! a [`PhaseStrategy`]. Planning is pure apart from ledger reads and optional
//! [`CatalogProbe`] lookups, and produces an immutable [`vdp_vaultspeed_domain::SourcePlan`].

mod decide;
mod error;
mod guard;
mod lifespan;
mod nature;
mod phasing;
mod policy;
mod probe;
mod staleness;

pub use decide::Planner;
pub use error::PlanError;
pub use guard::{DowngradeGuard, ExecutionGuard, RebuildGuard};
pub use nature::GroupNature;
pub use phasing::{
    ArtifactPhases, DdlPhases, EtlPhases, PhaseStrategy, PmlPhases, SequentialPhasing,
};
pub use policy::{
    AlwaysRunPolicy, DecisionContext, ExclusionPolicy, IdempotencyPolicy, OnceOnlyPolicy,
    ReplaceablePolicy, StandaloneDrops,
};
pub use probe::{CatalogProbe, ProbeError, ProbeVerdict};

#[cfg(test)]
mod tests;
