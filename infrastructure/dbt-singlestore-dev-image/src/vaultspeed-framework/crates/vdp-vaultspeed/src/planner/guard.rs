//! Guards veto an `Execute` verdict when executing would be unsafe. They are consulted
//! only after the decision chain chose to execute, so they never turn a harmless skip
//! into a refusal.

use std::fmt::Debug;

use vdp_vaultspeed_domain::{
    Decision, GenerationName, ObjectType, SourceKind, StatementGroup, StatementKind,
};

use crate::planner::lifespan::same_change;
use crate::planner::{DecisionContext, PlanError};

/// A veto on executing a group.
pub trait ExecutionGuard: Send + Sync + Debug {
    fn name(&self) -> &'static str;

    /// `Some(reason)` refuses the group.
    fn veto(
        &self,
        group: &StatementGroup,
        decision: &Decision,
        ctx: DecisionContext<'_>,
    ) -> Result<Option<String>, PlanError>;
}

/// Refuses to drop and re-create a table that holds history (the raw vault, order
/// number 5) with a different definition.
///
/// VaultSpeed delivers changes to existing raw-vault tables as `ALTER`s in delta (COMP)
/// releases. A *full* DDL generation instead emits `DROP TABLE` + `CREATE TABLE`, and
/// executing that on a populated satellite destroys its history; the Python framework
/// did exactly that. New tables (never created in this environment) and tables that
/// are gone (dropped or renamed away) are not affected.
#[derive(Debug, Clone)]
pub struct RebuildGuard {
    protected_order_numbers: Vec<u32>,
}

impl RebuildGuard {
    pub const fn new(protected_order_numbers: Vec<u32>) -> Self {
        Self {
            protected_order_numbers,
        }
    }
}

impl ExecutionGuard for RebuildGuard {
    fn name(&self) -> &'static str {
        "rebuild-guard"
    }

    fn veto(
        &self,
        group: &StatementGroup,
        _: &Decision,
        ctx: DecisionContext<'_>,
    ) -> Result<Option<String>, PlanError> {
        let protected = group
            .info
            .order
            .first()
            .is_some_and(|n| self.protected_order_numbers.contains(&n));
        let head = group.head();
        let is_table_rebuild = matches!(
            head.kind,
            StatementKind::Create {
                object: ObjectType::Table,
                ..
            }
        ) && group.statements.iter().any(|s| {
            matches!(
                s.kind,
                StatementKind::Drop {
                    object: ObjectType::Table,
                    ..
                }
            ) && s
                .target
                .as_ref()
                .zip(head.target.as_ref())
                .is_some_and(|(a, b)| a.same_name(b))
        });
        let Some(key) = head
            .definition_key()
            .filter(|_| protected && is_table_rebuild)
        else {
            return Ok(None);
        };
        let Some(current) = ctx.ledger.latest_for_object(&key.to_string())? else {
            return Ok(None); // never created here: a first deployment
        };
        // A table that lives under this name because another one was renamed to it holds that one's data.
        let key_text = key.to_string();
        if current.ends_definition_of(&key_text)
            || !current.begins_definition_of(&key_text)
            || same_change(&current, head)
        {
            return Ok(None);
        }
        Ok(Some(format!(
            "{} exists (ledger #{}) and this generation would DROP and re-create it with a \
             different definition, destroying its data; deliver the change as an ALTER \
             (COMP release) or remove order {} from planning.protected_rebuild_order_numbers",
            key.object.qualified_name(),
            current.sequence,
            group.info.order
        )))
    }
}

/// Refuses to let an older generation overwrite state a newer generation produced.
///
/// Release branches are merged into the environment branches by CI as they come; a
/// release merged late would otherwise roll procedures, views, comments and tables back
/// to the definitions of an older generation. Only provenance from real runs counts
/// (imported legacy entries do not know their generation), and PML is exempt.
#[derive(Debug, Clone, Copy, Default)]
pub struct DowngradeGuard;

impl ExecutionGuard for DowngradeGuard {
    fn name(&self) -> &'static str {
        "downgrade-guard"
    }

    fn veto(
        &self,
        group: &StatementGroup,
        _: &Decision,
        ctx: DecisionContext<'_>,
    ) -> Result<Option<String>, PlanError> {
        let SourceKind::Generation(incoming) = ctx.source else {
            return Ok(None);
        };
        for statement in &group.statements {
            let Some(key) = statement.object_key() else {
                continue;
            };
            let Some(current) = ctx.ledger.latest_for_object(&key.to_string())? else {
                continue;
            };
            let newer = current
                .source
                .parse::<GenerationName>()
                .ok()
                .filter(|producer| current.is_ordered() && producer.id() > incoming.id());
            if let Some(producer) = newer.filter(|_| !same_change(&current, statement)) {
                return Ok(Some(format!(
                    "{key} was last changed by generation {producer} (ledger #{}); {incoming} is \
                     older and would roll it back. Deploy a newer generation, or set \
                     planning.allow_downgrades = true for a deliberate rollback",
                    current.sequence
                )));
            }
        }
        Ok(None)
    }
}
