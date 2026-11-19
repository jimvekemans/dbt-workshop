use vdp_vaultspeed_domain::{SourcePlan, SourceReport};

use crate::engine::milestone::MilestoneGuard;
use crate::executor::FailurePolicy;
use crate::sources::{DagFiles, DiscoveredSource};

/// What loading and planning one source produced.
#[derive(Debug)]
pub(super) enum Preparation {
    Planned {
        plan: SourcePlan,
        dags: DagFiles,
    },
    /// The files could not be read or interpreted; nothing can be planned.
    Unloadable {
        message: String,
    },
}

/// One source, loaded and planned against the ledger as it was at that moment, but not executed.
///
/// [`Engine::prepare_source`](crate::engine::Engine::prepare_source) builds it and
/// [`Engine::execute_source`](crate::engine::Engine::execute_source) consumes it, so a source can neither
/// be executed twice nor without having been planned. Planning reads the ledger, which every executed
/// source changes: prepare a source only after the previous one has been executed.
///
/// Dropping it unexecuted ends the source's milestone as `failed`.
#[derive(Debug)]
#[must_use = "a prepared source does nothing until it is executed"]
pub struct PreparedSource {
    pub(super) source: DiscoveredSource,
    pub(super) policy: FailurePolicy,
    pub(super) preparation: Preparation,
    pub(super) guard: MilestoneGuard,
}

impl PreparedSource {
    /// The source that was prepared.
    pub const fn source(&self) -> &DiscoveredSource {
        &self.source
    }

    /// What executing this source does when a group fails.
    pub const fn policy(&self) -> FailurePolicy {
        self.policy
    }

    /// The plan, unless the source could not be loaded.
    pub const fn plan(&self) -> Option<&SourcePlan> {
        match &self.preparation {
            Preparation::Planned { plan, .. } => Some(plan),
            Preparation::Unloadable { .. } => None,
        }
    }

    /// Why the source could not be loaded.
    pub fn load_error(&self) -> Option<&str> {
        match &self.preparation {
            Preparation::Unloadable { message } => Some(message),
            Preparation::Planned { .. } => None,
        }
    }
}

/// A source after execution: its report and whether the run must stop.
#[derive(Debug)]
pub struct ExecutedSource {
    pub report: SourceReport,
    pub halts_run: bool,
}
