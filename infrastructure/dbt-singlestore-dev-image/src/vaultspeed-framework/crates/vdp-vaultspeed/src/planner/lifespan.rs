//! "Is this CREATE still what lives under the name?"
//!
//! A ledger entry for a CREATE proves it ran once. Names get reused, though: a table is renamed away
//! and a later release creates a table with the old name again, or the table goes away and comes back
//! through a rename round trip. This module reads the object's lifespan (see
//! [`vdp_vaultspeed_domain::current_lifespan`]) and turns it into a verdict about one CREATE statement.

use vdp_vaultspeed_domain::{
    LedgerEntry, Lifespan, ObjectKey, ObjectType, Statement, StatementKind, current_lifespan,
};

use crate::ledger::LedgerReader;
use crate::planner::PlanError;

/// What the object's lifespan says about a CREATE statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionVerdict {
    /// The object living under the name began with this very statement (also after a rename round
    /// trip), so running it again is redundant. `sequence` is that CREATE's ledger entry.
    Standing { sequence: u64 },
    /// The statement ran, but the object it created has since been dropped or renamed away and nothing
    /// lives under the name: running it again re-creates it. `since` is the ending entry.
    Ended { since: u64 },
}

/// Whether two ledger-relevant identities are the same change (raw fingerprint when known, else the
/// effective one: all legacy state recorded).
pub(crate) fn same_change(entry: &LedgerEntry, statement: &Statement) -> bool {
    entry.raw_fp == Some(statement.raw_fp) || entry.effective_fp == statement.effective_fp
}

/// The lifespan of whatever lives under definition key `key`.
pub(crate) fn lifespan_of(
    ledger: &dyn LedgerReader,
    key: &ObjectKey,
) -> Result<Lifespan, PlanError> {
    current_lifespan(&key.to_string(), &mut |k| ledger.object_history(k)).map_err(PlanError::from)
}

/// The verdict for a CREATE of a table, view, procedure or function; `None` for anything else, and
/// whenever the ledger cannot settle the question (no history, only imported entries, a different
/// definition): the caller then applies its ordinary rules.
pub(crate) fn definition_verdict(
    ledger: &dyn LedgerReader,
    statement: &Statement,
) -> Result<Option<DefinitionVerdict>, PlanError> {
    let StatementKind::Create {
        object: ObjectType::Table | ObjectType::View | ObjectType::Procedure | ObjectType::Function,
        ..
    } = statement.kind
    else {
        return Ok(None);
    };
    let Some(key) = statement.definition_key() else {
        return Ok(None);
    };
    let span = lifespan_of(ledger, &key)?;
    if span.lives {
        return Ok(span
            .origin
            .filter(|origin| same_change(origin, statement))
            .map(|origin| DefinitionVerdict::Standing {
                sequence: origin.sequence,
            }));
    }
    let Some(ended_by) = span.ended_by else {
        return Ok(None);
    };
    let key_text = key.to_string();
    let created_before = ledger.object_history(&key_text)?.iter().any(|entry| {
        entry.sequence < ended_by.sequence
            && !entry.is_rename()
            && entry.begins_definition_of(&key_text)
            && same_change(entry, statement)
    });
    Ok(created_before.then_some(DefinitionVerdict::Ended {
        since: ended_by.sequence,
    }))
}
