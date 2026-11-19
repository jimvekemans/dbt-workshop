use std::collections::HashMap;
use std::sync::RwLock;

use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry};

use crate::ledger::{LedgerError, LedgerReader, LedgerSink, LedgerWriter};

/// In-memory ledger with lookup indexes.
///
/// Built with a default database ([`MemoryLedger::from_entries_in`]) it qualifies the object keys of
/// legacy entries that were recorded without one (`relation:x` becomes `relation:edw.x`), so that
/// they match the keys of database-qualified statements. Only the in-memory copy changes; whatever
/// the entries were loaded from is never rewritten.
#[derive(Debug, Default)]
pub struct MemoryLedger {
    state: RwLock<State>,
}

#[derive(Debug, Default)]
struct State {
    entries: Vec<LedgerEntry>,
    last_sequence: u64,
    /// Indexes point at the latest *successful* entry.
    by_raw: HashMap<Fingerprint, usize>,
    by_effective: HashMap<Fingerprint, usize>,
    by_object: HashMap<String, usize>,
    /// Successful entries per object key in insertion (= sequence) order; a rename is listed under its
    /// source and its target.
    history: HashMap<String, Vec<usize>>,
    /// Database that unqualified object keys belong to; `None` leaves keys as they are.
    database: Option<String>,
}

impl State {
    fn insert(&mut self, mut entry: LedgerEntry) {
        if let Some(database) = &self.database {
            entry.object_key = entry.object_key.map(|k| qualify_key(&k, database));
            entry.renamed_to = entry.renamed_to.map(|k| qualify_key(&k, database));
        }
        let index = self.entries.len();
        self.last_sequence = self.last_sequence.max(entry.sequence);
        if entry.outcome.counts_as_applied() {
            if let Some(raw) = entry.raw_fp {
                self.by_raw.insert(raw, index);
            }
            self.by_effective.insert(entry.effective_fp, index);
            let keys = [&entry.object_key, &entry.renamed_to];
            for (position, key) in keys.into_iter().flatten().enumerate() {
                if position == 1 && entry.object_key.as_ref() == Some(key) {
                    continue; // a rename onto itself is one subject
                }
                self.by_object.insert(key.clone(), index);
                self.history.entry(key.clone()).or_default().push(index);
            }
        }
        self.entries.push(entry);
    }
}

/// `relation:x#comment` -> `relation:db.x#comment`; keys that already name a database, or whose family
/// has no database (databases, indexes), are returned unchanged.
pub(crate) fn qualify_key(key: &str, database: &str) -> String {
    let Some((family, rest)) = key.split_once(':') else {
        return key.to_owned();
    };
    if !matches!(family, "relation" | "procedure" | "function") || database.is_empty() {
        return key.to_owned();
    }
    let (name, facet) = rest
        .split_once('#')
        .map_or((rest, ""), |(n, _)| (n, &rest[n.len()..]));
    if name.contains('.') {
        key.to_owned()
    } else {
        format!("{family}:{database}.{name}{facet}")
    }
}

impl MemoryLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// A ledger whose first assigned sequence follows `offset` (used by overlays so
    /// sequences stay unique across base and overlay).
    pub fn starting_after(offset: u64) -> Self {
        let ledger = Self::default();
        ledger.state.write().expect("fresh lock").last_sequence = offset;
        ledger
    }

    /// Loads already-sequenced entries (e.g. from a file or table) in order.
    pub fn from_entries(entries: impl IntoIterator<Item = LedgerEntry>) -> Self {
        Self::from_entries_in("", entries)
    }

    /// Like [`MemoryLedger::from_entries`], qualifying unqualified object keys (and rename targets) with
    /// `database`, the environment's default database. Empty means unknown: keys stay as they are.
    pub fn from_entries_in(database: &str, entries: impl IntoIterator<Item = LedgerEntry>) -> Self {
        let ledger = Self::default();
        {
            let mut state = ledger.state.write().expect("fresh lock");
            state.database = Some(database.trim().to_lowercase()).filter(|d| !d.is_empty());
            for entry in entries {
                state.insert(entry);
            }
        }
        ledger
    }

    fn read<T>(&self, f: impl FnOnce(&State) -> T) -> Result<T, LedgerError> {
        self.state
            .read()
            .map(|s| f(&s))
            .map_err(|_| LedgerError::Poisoned)
    }
}

impl LedgerReader for MemoryLedger {
    fn find_applied(
        &self,
        raw: Option<&Fingerprint>,
        effective: &Fingerprint,
    ) -> Result<Option<LedgerEntry>, LedgerError> {
        self.read(|s| {
            raw.and_then(|fp| s.by_raw.get(fp))
                .or_else(|| s.by_effective.get(effective))
                .map(|&i| s.entries[i].clone())
        })
    }

    fn latest_for_object(&self, object_key: &str) -> Result<Option<LedgerEntry>, LedgerError> {
        self.read(|s| s.by_object.get(object_key).map(|&i| s.entries[i].clone()))
    }

    fn object_history(&self, object_key: &str) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.read(|s| {
            s.history
                .get(object_key)
                .map(|indexes| indexes.iter().map(|&i| s.entries[i].clone()).collect())
                .unwrap_or_default()
        })
    }

    fn last_sequence(&self) -> Result<u64, LedgerError> {
        self.read(|s| s.last_sequence)
    }

    fn entries(&self) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.read(|s| s.entries.clone())
    }
}

impl LedgerWriter for MemoryLedger {
    fn record(&self, mut entry: LedgerEntry) -> Result<LedgerEntry, LedgerError> {
        let mut state = self.state.write().map_err(|_| LedgerError::Poisoned)?;
        entry.sequence = state.last_sequence + 1;
        state.insert(entry.clone());
        Ok(entry)
    }
}

impl LedgerSink for MemoryLedger {
    fn append(&self, entry: &LedgerEntry) -> Result<(), LedgerError> {
        let mut state = self.state.write().map_err(|_| LedgerError::Poisoned)?;
        state.insert(entry.clone());
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use chrono::Utc;
    use vdp_vaultspeed_domain::{LedgerOutcome, RunId};

    use super::*;

    pub(crate) fn entry(raw: Option<&str>, effective: &str, outcome: LedgerOutcome) -> LedgerEntry {
        LedgerEntry {
            sequence: 0,
            run_id: RunId::from_string("test"),
            recorded_at: Utc::now(),
            source: "1_DDL".into(),
            job: "JOB".into(),
            rel_path: "x.sql".into(),
            object_key: Some("relation:s.t".into()),
            statement_kind: "ALTER TABLE ADD COLUMN".into(),
            raw_fp: raw.map(Fingerprint::of),
            effective_fp: Fingerprint::of(effective),
            outcome,
            detail: None,
            git_sha: None,
            renamed_to: None,
        }
    }

    #[test]
    fn finds_by_raw_then_effective() {
        let ledger = MemoryLedger::new();
        ledger
            .record(entry(Some("r1"), "e1", LedgerOutcome::Applied))
            .unwrap();
        ledger
            .record(entry(None, "e2", LedgerOutcome::Imported))
            .unwrap();

        let by_raw = ledger
            .find_applied(Some(&Fingerprint::of("r1")), &Fingerprint::of("zz"))
            .unwrap();
        assert_eq!(by_raw.unwrap().sequence, 1);
        let by_effective = ledger
            .find_applied(Some(&Fingerprint::of("new")), &Fingerprint::of("e2"))
            .unwrap();
        assert_eq!(by_effective.unwrap().sequence, 2);
        assert!(
            ledger
                .find_applied(None, &Fingerprint::of("e3"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn failures_never_count_as_applied() {
        let ledger = MemoryLedger::new();
        ledger
            .record(entry(Some("r"), "e", LedgerOutcome::Failed))
            .unwrap();
        assert!(
            ledger
                .find_applied(Some(&Fingerprint::of("r")), &Fingerprint::of("e"))
                .unwrap()
                .is_none()
        );
        assert!(ledger.latest_for_object("relation:s.t").unwrap().is_none());
        assert_eq!(ledger.entries().unwrap().len(), 1);
    }

    #[test]
    fn latest_object_version_wins() {
        let ledger = MemoryLedger::new();
        ledger
            .record(entry(Some("v1"), "v1", LedgerOutcome::Applied))
            .unwrap();
        ledger
            .record(entry(Some("v2"), "v2", LedgerOutcome::Applied))
            .unwrap();
        let latest = ledger.latest_for_object("relation:s.t").unwrap().unwrap();
        assert_eq!(latest.raw_fp, Some(Fingerprint::of("v2")));
        assert_eq!(ledger.last_sequence().unwrap(), 2);
    }

    fn rename(from: &str, to: &str) -> LedgerEntry {
        LedgerEntry {
            object_key: Some(from.into()),
            statement_kind: "ALTER TABLE RENAME".into(),
            renamed_to: Some(to.into()),
            ..entry(
                Some(&format!("{from}>{to}")),
                &format!("{from}>{to}"),
                LedgerOutcome::Applied,
            )
        }
    }

    #[test]
    fn a_rename_is_indexed_under_both_names() {
        let ledger = MemoryLedger::new();
        ledger
            .record(rename("relation:s.a", "relation:s.y"))
            .unwrap();
        for key in ["relation:s.a", "relation:s.y"] {
            assert_eq!(ledger.latest_for_object(key).unwrap().unwrap().sequence, 1);
            assert_eq!(ledger.object_history(key).unwrap().len(), 1);
        }
    }

    #[test]
    fn object_history_is_in_sequence_order_and_skips_failures() {
        let ledger = MemoryLedger::new();
        ledger
            .record(entry(Some("a"), "a", LedgerOutcome::Applied))
            .unwrap();
        ledger
            .record(entry(Some("b"), "b", LedgerOutcome::Failed))
            .unwrap();
        ledger
            .record(entry(Some("c"), "c", LedgerOutcome::Applied))
            .unwrap();
        let sequences: Vec<_> = ledger
            .object_history("relation:s.t")
            .unwrap()
            .iter()
            .map(|e| e.sequence)
            .collect();
        assert_eq!(sequences, vec![1, 3]);
    }

    #[test]
    fn legacy_unqualified_keys_are_found_by_qualified_lookups() {
        let mut create = entry(None, "c", LedgerOutcome::Imported);
        create.object_key = Some("relation:x".into());
        let mut comment = entry(None, "d", LedgerOutcome::Imported);
        comment.object_key = Some("relation:x#comment".into());
        let mut moved = rename("relation:x", "relation:x_old");
        moved.sequence = 3;
        let ledger = MemoryLedger::from_entries_in("EDW", [create, comment, moved]);

        assert!(
            ledger
                .latest_for_object("relation:edw.x")
                .unwrap()
                .is_some()
        );
        assert!(
            ledger
                .latest_for_object("relation:edw.x#comment")
                .unwrap()
                .is_some()
        );
        assert!(ledger.latest_for_object("relation:x").unwrap().is_none());
        let renamed = ledger
            .latest_for_object("relation:edw.x_old")
            .unwrap()
            .unwrap();
        assert_eq!(renamed.object_key.as_deref(), Some("relation:edw.x"));
        // Without a database the keys are untouched.
        let plain = MemoryLedger::from_entries([entry(None, "c", LedgerOutcome::Imported)]);
        assert!(plain.latest_for_object("relation:s.t").unwrap().is_some());
    }

    #[test]
    fn keys_are_qualified_only_when_they_need_it() {
        assert_eq!(qualify_key("relation:x", "d"), "relation:d.x");
        assert_eq!(qualify_key("procedure:p#x", "d"), "procedure:d.p#x");
        assert_eq!(
            qualify_key("relation:s.x#column:c", "d"),
            "relation:s.x#column:c"
        );
        assert_eq!(
            qualify_key("relation:x#column:c", "d"),
            "relation:d.x#column:c"
        );
        assert_eq!(qualify_key("database:x", "d"), "database:x");
        assert_eq!(qualify_key("relation:x", ""), "relation:x");
    }
}
