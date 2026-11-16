//! "It ran" is not "it is still in effect".
//!
//! A ledger entry proves a statement executed once. Its effect can be undone later by
//! another change to the same state: the column was dropped, the table renamed away or
//! rebuilt, the rows wiped by a rebuild. This module finds such later changes.

use std::collections::HashSet;

use crate::ledger::LedgerReader;
use vdp_vaultspeed_domain::{
    AlterOp, Facet, Fingerprint, LedgerEntry, Statement, StatementGroup, StatementKind,
};

use crate::planner::PlanError;

/// Fingerprints of a group's own statements; entries matching them never supersede the
/// group (the DROP of a DROP + CREATE pair is followed by its own CREATE).
pub(crate) fn own_fingerprints(group: &StatementGroup) -> HashSet<Fingerprint> {
    group
        .statements
        .iter()
        .flat_map(|s| [s.raw_fp, s.effective_fp])
        .collect()
}

/// The sequence of the earliest-found later change that replaced the state `applied`
/// produced for `statement`, or `None` when the effect still stands.
///
/// Two histories are consulted:
/// - the statement's own state key, where only a change that *undoes* this one counts
///   (see [`undoes`]): a second INSERT into a table does not undo the first;
/// - for anything but the definition itself, the object's definition: a later CREATE,
///   DROP or RENAME of the table voids its columns, comment and rows.
///
/// Only ordered entries (from real runs) can supersede; imported legacy entries have no
/// trustworthy order across files.
pub(crate) fn superseded_since(
    ledger: &dyn LedgerReader,
    statement: &Statement,
    applied: &LedgerEntry,
    own: &HashSet<Fingerprint>,
) -> Result<Option<u64>, PlanError> {
    let Some(own_key) = statement.object_key() else {
        return Ok(None);
    };
    let mut keys = vec![(own_key.to_string(), false)];
    if !own_key.is_definition() {
        keys.extend(statement.definition_key().map(|k| (k.to_string(), true)));
    }
    for (key, is_definition_of_target) in keys {
        let Some(latest) = ledger.latest_for_object(&key)? else {
            continue;
        };
        let belongs_to_group =
            latest.raw_fp.is_some_and(|fp| own.contains(&fp)) || own.contains(&latest.effective_fp);
        let later = latest.sequence > applied.sequence && latest.is_ordered() && !belongs_to_group;
        if later && (is_definition_of_target || undoes(&latest, &own_key.facet, &statement.kind)) {
            return Ok(Some(latest.sequence));
        }
    }
    Ok(None)
}

/// Whether a later change on the same state key undoes `earlier`.
///
/// - Definition and comment hold one value: any different later change replaces it.
/// - A column: `ADD`/`CHANGE` are undone by `DROP COLUMN`; `DROP` by `ADD`/`CHANGE`;
///   `MODIFY` by any later change to the column (its type was redefined again).
/// - Rows accumulate: only `TRUNCATE` (or replacing the table) undoes an `INSERT`.
/// - Other alterations (indexes, ...) accumulate.
pub(crate) fn undoes(later: &LedgerEntry, facet: &Facet, earlier: &StatementKind) -> bool {
    let kind = later.statement_kind.as_str();
    match facet {
        Facet::Definition | Facet::Comment => true,
        Facet::Column(_) => match earlier {
            StatementKind::Alter {
                op: AlterOp::DropColumn { .. },
            } => {
                matches!(kind, "ALTER TABLE ADD COLUMN" | "ALTER TABLE CHANGE COLUMN")
            }
            StatementKind::Alter {
                op: AlterOp::ModifyColumn { .. },
            } => true,
            _ => matches!(
                kind,
                "ALTER TABLE DROP COLUMN" | "ALTER TABLE CHANGE COLUMN"
            ),
        },
        Facet::Data => kind == "TRUNCATE",
        Facet::Alteration => false,
    }
}
