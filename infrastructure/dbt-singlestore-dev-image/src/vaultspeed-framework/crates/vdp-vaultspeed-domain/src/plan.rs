use serde::{Deserialize, Serialize};

use crate::{Decision, SourceKind, StatementGroup};

/// A group together with the planner's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedGroup {
    pub group: StatementGroup,
    pub decision: Decision,
}

/// An ordered batch of groups that must complete before the next phase starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanPhase {
    pub name: String,
    pub groups: Vec<PlannedGroup>,
}

/// The immutable plan for one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePlan {
    pub source: SourceKind,
    pub phases: Vec<PlanPhase>,
}

/// How many groups a plan executes and how many it skips (a refused group counts as skipped).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCounts {
    pub execute: usize,
    pub skip: usize,
}

impl SourcePlan {
    /// Every group of the plan with the phase it is in, in order.
    pub fn groups(&self) -> impl Iterator<Item = (&PlanPhase, &PlannedGroup)> {
        self.phases
            .iter()
            .flat_map(|phase| phase.groups.iter().map(move |g| (phase, g)))
    }

    /// Counts the groups that execute and the others.
    pub fn counts(&self) -> PlanCounts {
        self.groups()
            .fold(PlanCounts::default(), |mut counts, (_, planned)| {
                if planned.decision.is_execute() {
                    counts.execute += 1;
                } else {
                    counts.skip += 1;
                }
                counts
            })
    }
}
