use vdp_vaultspeed_domain::{AlterOp, ObjectKey, StatementGroup, StatementKind};

/// How a group behaves when it is applied again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupNature {
    /// Redefines an object wholesale without data loss; re-run when its definition
    /// differs from the last applied version.
    Replaceable(ObjectKey),
    /// Must run at most once (DDL on tables, DML).
    OnceOnly,
}

impl GroupNature {
    /// - `CREATE OR REPLACE VIEW|PROCEDURE|FUNCTION`
    /// - `CREATE VIEW|PROCEDURE|FUNCTION x` preceded in the group by `DROP ... x`
    /// - `ALTER TABLE ... COMMENT` (the comment is a replaceable facet of the table)
    pub fn of(group: &StatementGroup) -> Self {
        let head = group.head();
        let replaceable = match &head.kind {
            StatementKind::Create {
                object, or_replace, ..
            } if object.is_replaceable() => *or_replace || dropped_before_create(group),
            StatementKind::Alter {
                op: AlterOp::Comment,
            } => group.statements.len() == 1,
            _ => false,
        };
        match head.object_key() {
            Some(key) if replaceable => Self::Replaceable(key),
            _ => Self::OnceOnly,
        }
    }
}

fn dropped_before_create(group: &StatementGroup) -> bool {
    let head = group.head();
    let Some(target) = &head.target else {
        return false;
    };
    group
        .statements
        .iter()
        .any(|s| s.kind.is_drop() && s.target.as_ref().is_some_and(|t| t.same_name(target)))
}
