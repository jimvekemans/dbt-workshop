use std::collections::HashMap;
use std::sync::Arc;

use crate::ledger::LedgerReader;
use vdp_vaultspeed_domain::{
    Decision, ExecuteReason, Fingerprint, GroupId, PlanPhase, PlannedGroup, SkipReason, SourceKind,
    SourcePlan, StatementGroup,
};

use crate::planner::{
    DecisionContext, ExecutionGuard, GroupNature, IdempotencyPolicy, PhaseStrategy, PlanError,
};

/// Builds the plan for one source.
#[derive(Debug, Clone)]
pub struct Planner {
    policies: Vec<Arc<dyn IdempotencyPolicy>>,
    guards: Vec<Arc<dyn ExecutionGuard>>,
    phasing: Arc<dyn PhaseStrategy>,
}

impl Planner {
    /// `policies` are consulted in order and the first verdict wins; `guards` may then
    /// veto an `Execute` verdict.
    pub fn new(
        policies: Vec<Arc<dyn IdempotencyPolicy>>,
        guards: Vec<Arc<dyn ExecutionGuard>>,
        phasing: Arc<dyn PhaseStrategy>,
    ) -> Self {
        Self {
            policies,
            guards,
            phasing,
        }
    }

    pub fn plan(
        &self,
        source: &SourceKind,
        groups: Vec<StatementGroup>,
        ledger: &dyn LedgerReader,
    ) -> Result<SourcePlan, PlanError> {
        let ctx = DecisionContext { source, ledger };
        let mut executed_by: HashMap<Fingerprint, GroupId> = HashMap::new();
        let mut phases = Vec::new();
        for (name, groups) in self.phasing.phases(groups) {
            let mut planned = Vec::with_capacity(groups.len());
            for group in groups {
                let mut decision = self.decide(&group, ctx)?;
                if let Some(first) = duplicate_of(&group, &decision, &executed_by) {
                    decision = Decision::Skip(SkipReason::DuplicateInPlan {
                        group: first.to_string(),
                    });
                } else if decision.is_execute() {
                    for statement in &group.statements {
                        executed_by
                            .entry(statement.raw_fp)
                            .or_insert_with(|| group.id.clone());
                    }
                }
                planned.push(PlannedGroup { group, decision });
            }
            phases.push(PlanPhase {
                name,
                groups: planned,
            });
        }
        Ok(SourcePlan {
            source: source.clone(),
            phases,
        })
    }

    fn decide(
        &self,
        group: &StatementGroup,
        ctx: DecisionContext<'_>,
    ) -> Result<vdp_vaultspeed_domain::Decision, PlanError> {
        let mut verdict = None;
        for policy in &self.policies {
            if let Some(decision) = policy.decide(group, ctx)? {
                verdict = Some(decision);
                break;
            }
        }
        let decision = verdict.ok_or_else(|| PlanError::Undecided(group.id.to_string()))?;
        if decision.is_execute() {
            for guard in &self.guards {
                if let Some(reason) = guard.veto(group, &decision, ctx)? {
                    return Ok(Decision::Refuse {
                        reason: format!("{}: {reason}", guard.name()),
                    });
                }
            }
        }
        Ok(decision)
    }
}

/// A run-once group whose every statement an earlier group of this plan already
/// executes (the same ALTER in two jobs of one COMP, the same seed INSERT twice). Running
/// it again would at best fail and at worst duplicate rows or wipe a fresh table.
/// Replaceable redefinitions and staged PML are exempt: re-running them is harmless or
/// intended.
fn duplicate_of<'a>(
    group: &StatementGroup,
    decision: &Decision,
    executed_by: &'a HashMap<Fingerprint, GroupId>,
) -> Option<&'a GroupId> {
    let Decision::Execute(
        ExecuteReason::New | ExecuteReason::Stale { .. } | ExecuteReason::Vanished { .. },
    ) = decision
    else {
        return None;
    };
    if GroupNature::of(group) != GroupNature::OnceOnly {
        return None;
    }
    let mut first = None;
    for statement in &group.statements {
        let earlier = executed_by.get(&statement.raw_fp)?;
        first.get_or_insert(earlier);
    }
    first
}
