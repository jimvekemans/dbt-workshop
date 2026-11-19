use std::fmt::Debug;
use std::sync::Arc;

use crate::ledger::LedgerReader;
use vdp_vaultspeed_domain::{
    ArtifactKind, Decision, ExecuteReason, ObjectType, SkipReason, SourceKind, Statement,
    StatementGroup, StatementKind,
};

use crate::planner::lifespan::{DefinitionVerdict, definition_verdict};
use crate::planner::staleness::{own_fingerprints, superseded_since};
use crate::planner::{CatalogProbe, GroupNature, PlanError, ProbeVerdict};

/// What a policy may consult.
#[derive(Debug, Clone, Copy)]
pub struct DecisionContext<'a> {
    pub source: &'a SourceKind,
    pub ledger: &'a dyn LedgerReader,
}

/// One rule of the decision chain. Returns `None` when it has no opinion.
pub trait IdempotencyPolicy: Send + Sync + Debug {
    fn name(&self) -> &'static str;
    fn decide(
        &self,
        group: &StatementGroup,
        ctx: DecisionContext<'_>,
    ) -> Result<Option<Decision>, PlanError>;
}

/// What to do with a DDL `DROP` that is not followed by a `CREATE` of the same object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandaloneDrops {
    /// Skip, visibly. The Python implementation silently discarded these statements
    /// (e.g. the "drop dependent views" section of ALTER jobs); executing them would
    /// drop views nothing recreates, so parity is the safe default.
    Skip,
    Execute,
}

/// Configured exclusions; always first in the chain.
#[derive(Debug, Clone)]
pub struct ExclusionPolicy {
    job_substrings: Vec<String>,
    path_substrings: Vec<String>,
    standalone_drops: StandaloneDrops,
}

impl ExclusionPolicy {
    pub const fn new(
        job_substrings: Vec<String>,
        path_substrings: Vec<String>,
        standalone_drops: StandaloneDrops,
    ) -> Self {
        Self {
            job_substrings,
            path_substrings,
            standalone_drops,
        }
    }
}

impl IdempotencyPolicy for ExclusionPolicy {
    fn name(&self) -> &'static str {
        "exclusion"
    }

    fn decide(
        &self,
        group: &StatementGroup,
        _: DecisionContext<'_>,
    ) -> Result<Option<Decision>, PlanError> {
        let excluded = |rule: String| Ok(Some(Decision::Skip(SkipReason::Excluded { rule })));
        if let Some(needle) = self
            .job_substrings
            .iter()
            .find(|n| group.info.job.as_str().contains(n.as_str()))
        {
            return excluded(format!("job name contains `{needle}`"));
        }
        let path = format!("/{}", group.info.rel_path);
        if let Some(needle) = self
            .path_substrings
            .iter()
            .find(|n| path.contains(n.as_str()))
        {
            return excluded(format!("path contains `{needle}`"));
        }
        let standalone_drop = group.info.artifact == ArtifactKind::Ddl
            && group.statements.len() == 1
            && group.statements[0].kind.is_drop();
        if standalone_drop && self.standalone_drops == StandaloneDrops::Skip {
            return excluded(
                "standalone DDL DROP (planning.standalone_drops = \"skip\")".to_owned(),
            );
        }
        Ok(None)
    }
}

/// Artifacts whose staging is the trigger: they run every time they are present.
#[derive(Debug, Clone)]
pub struct AlwaysRunPolicy {
    artifacts: Vec<ArtifactKind>,
}

impl AlwaysRunPolicy {
    pub const fn new(artifacts: Vec<ArtifactKind>) -> Self {
        Self { artifacts }
    }
}

impl IdempotencyPolicy for AlwaysRunPolicy {
    fn name(&self) -> &'static str {
        "always-run"
    }

    fn decide(
        &self,
        group: &StatementGroup,
        _: DecisionContext<'_>,
    ) -> Result<Option<Decision>, PlanError> {
        Ok(self
            .artifacts
            .contains(&group.info.artifact)
            .then_some(Decision::Execute(ExecuteReason::AlwaysRun)))
    }
}

/// A group that the ledger says is applied, while the catalog shows its table or view is gone (renamed
/// or dropped outside the framework): run it again. Only asked when a probe is configured.
fn vanished(
    probe: Option<&Arc<dyn CatalogProbe>>,
    head: &Statement,
) -> Result<Option<Decision>, PlanError> {
    let (Some(probe), StatementKind::Create { object, .. }, Some(target)) =
        (probe, &head.kind, &head.target)
    else {
        return Ok(None);
    };
    if !matches!(object, ObjectType::Table | ObjectType::View) {
        return Ok(None);
    }
    Ok((probe.exists(target)? == Some(false)).then(|| {
        Decision::Execute(ExecuteReason::Vanished {
            detail: format!("{target} is not in the catalog"),
        })
    }))
}

/// Views, procedures, functions and table comments: run when the definition changed.
#[derive(Debug, Clone, Default)]
pub struct ReplaceablePolicy {
    probe: Option<Arc<dyn CatalogProbe>>,
}

impl ReplaceablePolicy {
    pub const fn new(probe: Option<Arc<dyn CatalogProbe>>) -> Self {
        Self { probe }
    }
}

impl IdempotencyPolicy for ReplaceablePolicy {
    fn name(&self) -> &'static str {
        "replaceable"
    }

    fn decide(
        &self,
        group: &StatementGroup,
        ctx: DecisionContext<'_>,
    ) -> Result<Option<Decision>, PlanError> {
        let GroupNature::Replaceable(key) = GroupNature::of(group) else {
            return Ok(None);
        };
        let head = group.head();
        // The object this very definition created is gone (dropped, renamed away): bring it back.
        if let Some(DefinitionVerdict::Ended { since }) = definition_verdict(ctx.ledger, head)? {
            return Ok(Some(Decision::Execute(ExecuteReason::Stale { since })));
        }
        let key_text = key.to_string();
        let decision = match ctx.ledger.latest_for_object(&key_text)? {
            None => Decision::Execute(ExecuteReason::New),
            Some(latest) if latest.ends_definition_of(&key_text) => {
                Decision::Execute(ExecuteReason::Changed { previous: None })
            }
            Some(latest) => {
                let same_raw = latest.raw_fp.is_some_and(|fp| fp == head.raw_fp);
                if !(same_raw || latest.effective_fp == head.effective_fp) {
                    Decision::Execute(ExecuteReason::Changed {
                        previous: Some(latest.raw_fp.unwrap_or(latest.effective_fp)),
                    })
                } else if let Some(since) =
                    superseded_since(ctx.ledger, head, &latest, &own_fingerprints(group))?
                {
                    // e.g. an unchanged table comment after the table was rebuilt.
                    Decision::Execute(ExecuteReason::Stale { since })
                } else {
                    Decision::Skip(SkipReason::AlreadyApplied {
                        sequence: latest.sequence,
                    })
                }
            }
        };
        if matches!(decision, Decision::Skip(SkipReason::AlreadyApplied { .. }))
            && let Some(gone) = vanished(self.probe.as_ref(), head)?
        {
            return Ok(Some(gone));
        }
        Ok(Some(decision))
    }
}

/// Fallback for everything else: a group runs unless every statement is in the ledger
/// or, optionally, the catalog proves every missing statement is already in effect.
#[derive(Debug, Clone, Default)]
pub struct OnceOnlyPolicy {
    probe: Option<Arc<dyn CatalogProbe>>,
}

impl OnceOnlyPolicy {
    pub const fn new(probe: Option<Arc<dyn CatalogProbe>>) -> Self {
        Self { probe }
    }
}

impl IdempotencyPolicy for OnceOnlyPolicy {
    fn name(&self) -> &'static str {
        "once-only"
    }

    fn decide(
        &self,
        group: &StatementGroup,
        ctx: DecisionContext<'_>,
    ) -> Result<Option<Decision>, PlanError> {
        let own = own_fingerprints(group);
        let mut latest_sequence = 0;
        let mut stale_since = None;
        let mut never_applied = false;
        let mut missing = Vec::new();
        // The lifespan of a created object settles its CREATE first: the ledger entry of a CREATE that
        // ran before the table was renamed away and came back is still what lives there.
        let verdicts = group
            .statements
            .iter()
            .map(|s| definition_verdict(ctx.ledger, s))
            .collect::<Result<Vec<_>, _>>()?;
        let standing_creates: Vec<_> = group
            .statements
            .iter()
            .zip(&verdicts)
            .filter_map(|(s, v)| match v {
                Some(DefinitionVerdict::Standing { sequence }) => {
                    s.target.as_ref().map(|t| (t, *sequence))
                }
                _ => None,
            })
            .collect();
        for (statement, verdict) in group.statements.iter().zip(&verdicts) {
            // The DROP of a DROP + CREATE pair stands with its CREATE.
            let paired_drop = statement
                .kind
                .is_drop()
                .then(|| {
                    standing_creates.iter().find(|(target, _)| {
                        statement
                            .target
                            .as_ref()
                            .is_some_and(|t| t.same_name(target))
                    })
                })
                .flatten();
            match (verdict, paired_drop) {
                (Some(DefinitionVerdict::Standing { sequence }), _) | (_, Some((_, sequence))) => {
                    latest_sequence = latest_sequence.max(*sequence);
                    continue;
                }
                (Some(DefinitionVerdict::Ended { since }), _) => {
                    stale_since = Some(stale_since.map_or(*since, |s: u64| s.max(*since)));
                    missing.push(statement);
                    continue;
                }
                (None, None) => {}
            }
            if let Some(entry) = ctx
                .ledger
                .find_applied(Some(&statement.raw_fp), &statement.effective_fp)?
            {
                match superseded_since(ctx.ledger, statement, &entry, &own)? {
                    Some(since) => {
                        stale_since = Some(stale_since.map_or(since, |s: u64| s.max(since)));
                        missing.push(statement);
                    }
                    None => latest_sequence = latest_sequence.max(entry.sequence),
                }
            } else {
                never_applied = true;
                missing.push(statement);
            }
        }
        if missing.is_empty() {
            if let Some(gone) = vanished(self.probe.as_ref(), group.head())? {
                return Ok(Some(gone));
            }
            return Ok(Some(Decision::Skip(SkipReason::AlreadyApplied {
                sequence: latest_sequence,
            })));
        }

        // A DROP + CREATE pair is a deliberate rebuild: an existing table proves nothing.
        let rebuild = group
            .statements
            .iter()
            .any(|s| matches!(s.kind, StatementKind::Drop { .. }));
        if let (Some(probe), false) = (&self.probe, rebuild) {
            let mut evidence = Vec::with_capacity(missing.len());
            for statement in &missing {
                match probe.verify(statement)? {
                    ProbeVerdict::Holds(found) => evidence.push(found),
                    ProbeVerdict::Contradicts(detail) => {
                        return Ok(Some(Decision::Refuse {
                            reason: format!("{}: {detail}", statement.kind.label()),
                        }));
                    }
                    ProbeVerdict::Unknown => break,
                }
            }
            if evidence.len() == missing.len() {
                let evidence = evidence.swap_remove(0);
                return Ok(Some(Decision::Skip(SkipReason::Satisfied { evidence })));
            }
        }
        // "New" as soon as anything never ran; "stale" only when everything missing ran
        // before and was undone since.
        Ok(Some(Decision::Execute(match stale_since {
            Some(since) if !never_applied => ExecuteReason::Stale { since },
            _ => ExecuteReason::New,
        })))
    }
}
