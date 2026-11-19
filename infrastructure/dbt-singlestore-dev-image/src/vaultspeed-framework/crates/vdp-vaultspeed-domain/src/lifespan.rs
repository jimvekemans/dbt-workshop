use std::collections::HashSet;

use crate::LedgerEntry;

/// Rename chains are followed backwards, each hop strictly earlier in the ledger, so they end by
/// themselves; the bound only guards against a corrupt ledger.
const MAX_HOPS: usize = 256;

/// The life of whatever object currently lives under one name, read from the ledger.
///
/// A name is reused: `x` is created, renamed to `x_old`, and a later release creates `x` again. "Was this
/// CREATE applied?" is then the wrong question; "did the object living under `x` now start with this
/// CREATE?" is the right one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifespan {
    /// Whether the ledger shows something living under the name. Imported legacy entries prove nothing
    /// about order, so a name known only from them has `lives == false` and no `ended_by` either.
    pub lives: bool,
    /// The CREATE that started the living object, following renames back to the name it was first created
    /// under. `None` when nothing lives there or the origin is unknown.
    pub origin: Option<LedgerEntry>,
    /// When nothing lives under the name: the DROP or rename-away that ended its last lifespan.
    pub ended_by: Option<LedgerEntry>,
}

impl Lifespan {
    const fn unknown() -> Self {
        Self {
            lives: false,
            origin: None,
            ended_by: None,
        }
    }
}

/// The current lifespan under definition key `key`. `history(key)` returns the successful ledger entries
/// indexed under a key in sequence order (including renames into it).
///
/// The history is walked backwards. The first ordered entry that ends the name's existence settles that
/// nothing lives there (if nothing was found before it); a CREATE or rename into the name means something
/// lives there, and a rename's origin is the origin of the object under its old name *before the rename*.
/// Only ordered entries take part: imported legacy entries never prove an order.
pub fn current_lifespan<E>(
    key: &str,
    history: &mut dyn FnMut(&str) -> Result<Vec<LedgerEntry>, E>,
) -> Result<Lifespan, E> {
    walk(key, u64::MAX, history, &mut HashSet::new(), 0)
}

fn walk<E>(
    key: &str,
    before: u64,
    history: &mut dyn FnMut(&str) -> Result<Vec<LedgerEntry>, E>,
    seen: &mut HashSet<(String, u64)>,
    hops: usize,
) -> Result<Lifespan, E> {
    if hops > MAX_HOPS || !seen.insert((key.to_owned(), before)) {
        return Ok(Lifespan::unknown());
    }
    let entries = history(key)?;
    let mut lives = false;
    let mut origin: Option<LedgerEntry> = None;
    for entry in entries
        .iter()
        .rev()
        .filter(|e| e.sequence < before && e.is_ordered())
    {
        if entry.ends_definition_of(key) {
            if lives {
                break; // an earlier lifespan; the current one is already known
            }
            return Ok(Lifespan {
                lives: false,
                origin: None,
                ended_by: Some(entry.clone()),
            });
        }
        if !entry.begins_definition_of(key) {
            continue;
        }
        lives = true;
        if entry.is_rename() {
            // The object arrived under this name by the rename, so its origin is the source's, even when a
            // later CREATE was seen first: with nothing ended in between, that CREATE created nothing (it was
            // satisfied, or `IF NOT EXISTS`).
            origin = match &entry.object_key {
                Some(source) => {
                    let before_rename = walk(source, entry.sequence, history, seen, hops + 1)?;
                    before_rename.origin.filter(|_| before_rename.lives)
                }
                None => None,
            };
            break;
        }
        // Walking backwards, the earliest CREATE since the last ending ends up as the origin.
        origin = Some(entry.clone());
    }
    Ok(Lifespan {
        lives,
        origin,
        ended_by: None,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{Fingerprint, LedgerOutcome, RunId};

    fn entry(
        sequence: u64,
        kind: &str,
        key: &str,
        renamed_to: Option<&str>,
        outcome: LedgerOutcome,
    ) -> LedgerEntry {
        LedgerEntry {
            sequence,
            run_id: RunId::from_string("t"),
            recorded_at: Utc::now(),
            source: "PML".into(),
            job: "J".into(),
            rel_path: "x.sql".into(),
            object_key: Some(key.into()),
            statement_kind: kind.into(),
            raw_fp: Some(Fingerprint::of(format!("{kind}{sequence}"))),
            effective_fp: Fingerprint::of(format!("{kind}{sequence}")),
            outcome,
            detail: None,
            git_sha: None,
            renamed_to: renamed_to.map(str::to_owned),
        }
    }

    fn lifespan(key: &str, all: &[LedgerEntry]) -> Lifespan {
        current_lifespan::<()>(key, &mut |k| {
            Ok(all
                .iter()
                .filter(|e| {
                    e.object_key.as_deref() == Some(k) || e.renamed_to.as_deref() == Some(k)
                })
                .cloned()
                .collect())
        })
        .unwrap()
    }

    const X: &str = "relation:s.x";
    const Y: &str = "relation:s.y";

    #[test]
    fn a_rename_round_trip_keeps_the_original_create_as_origin() {
        let ledger = [
            entry(1, "CREATE TABLE", X, None, LedgerOutcome::Applied),
            entry(2, "ALTER TABLE RENAME", X, Some(Y), LedgerOutcome::Applied),
            entry(3, "ALTER TABLE RENAME", Y, Some(X), LedgerOutcome::Applied),
        ];
        let span = lifespan(X, &ledger);
        assert!(span.lives);
        assert_eq!(span.origin.unwrap().sequence, 1);
        // The detour name is empty again.
        let y = lifespan(Y, &ledger);
        assert!(!y.lives);
        assert_eq!(y.ended_by.unwrap().sequence, 3);
    }

    #[test]
    fn a_renamed_object_takes_its_origin_along() {
        let ledger = [
            entry(1, "CREATE TABLE", X, None, LedgerOutcome::Applied),
            entry(2, "ALTER TABLE RENAME", X, Some(Y), LedgerOutcome::Applied),
        ];
        assert_eq!(lifespan(Y, &ledger).origin.unwrap().sequence, 1);
        let x = lifespan(X, &ledger);
        assert!(!x.lives);
        assert_eq!(x.ended_by.unwrap().sequence, 2);
    }

    #[test]
    fn recreating_after_a_drop_starts_a_new_lifespan() {
        let ledger = [
            entry(1, "CREATE TABLE", X, None, LedgerOutcome::Applied),
            entry(2, "DROP TABLE", X, None, LedgerOutcome::Applied),
            entry(3, "CREATE TABLE", X, None, LedgerOutcome::Applied),
        ];
        let span = lifespan(X, &ledger);
        assert!(span.lives);
        assert_eq!(span.origin.unwrap().sequence, 3);
    }

    #[test]
    fn imported_entries_prove_no_order() {
        let ledger = [
            entry(1, "CREATE TABLE", X, None, LedgerOutcome::Applied),
            entry(2, "DROP TABLE", X, None, LedgerOutcome::Imported),
        ];
        let span = lifespan(X, &ledger);
        assert!(span.lives, "an imported DROP cannot end the lifespan");
        assert!(span.ended_by.is_none());
    }

    #[test]
    fn rename_cycles_terminate() {
        // A corrupt ledger where the names rename into each other with the same sequence numbers.
        let ledger = [
            entry(5, "ALTER TABLE RENAME", X, Some(Y), LedgerOutcome::Applied),
            entry(5, "ALTER TABLE RENAME", Y, Some(X), LedgerOutcome::Applied),
        ];
        let span = lifespan(X, &ledger);
        assert!(span.origin.is_none());
    }
}

#[cfg(test)]
mod satisfied_create_tests {
    use chrono::Utc;

    use super::*;
    use crate::{Fingerprint, LedgerOutcome, RunId};

    fn entry(sequence: u64, kind: &str, key: &str, renamed_to: Option<&str>) -> LedgerEntry {
        LedgerEntry {
            sequence,
            run_id: RunId::from_string("t"),
            recorded_at: Utc::now(),
            source: "PML".into(),
            job: "J".into(),
            rel_path: "x.sql".into(),
            object_key: Some(key.into()),
            statement_kind: kind.into(),
            raw_fp: Some(Fingerprint::of(format!("{kind}{sequence}"))),
            effective_fp: Fingerprint::of(format!("{kind}{sequence}")),
            outcome: LedgerOutcome::Applied,
            detail: None,
            git_sha: None,
            renamed_to: renamed_to.map(str::to_owned),
        }
    }

    #[test]
    fn a_create_after_a_rename_in_does_not_hide_the_real_origin() {
        let all = [
            entry(1, "CREATE TABLE", "relation:s.y", None),
            entry(
                2,
                "ALTER TABLE RENAME",
                "relation:s.y",
                Some("relation:s.x"),
            ),
            // Satisfied: x already existed (it came from y).
            entry(3, "CREATE TABLE", "relation:s.x", None),
        ];
        let lifespan = current_lifespan::<()>("relation:s.x", &mut |k| {
            Ok(all
                .iter()
                .filter(|e| {
                    e.object_key.as_deref() == Some(k) || e.renamed_to.as_deref() == Some(k)
                })
                .cloned()
                .collect())
        })
        .unwrap();
        assert!(lifespan.lives);
        assert_eq!(lifespan.origin.map(|o| o.sequence), Some(1));
    }
}
