use std::collections::HashMap;
use std::sync::RwLock;

use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry};

use crate::ledger::{LedgerError, LedgerReader, LedgerSink, LedgerWriter};

/// In-memory ledger with lookup indexes.
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
}

impl State {
    fn insert(&mut self, entry: LedgerEntry) {
        let index = self.entries.len();
        self.last_sequence = self.last_sequence.max(entry.sequence);
        if entry.outcome.counts_as_applied() {
            if let Some(raw) = entry.raw_fp {
                self.by_raw.insert(raw, index);
            }
            self.by_effective.insert(entry.effective_fp, index);
            if let Some(key) = &entry.object_key {
                self.by_object.insert(key.clone(), index);
            }
        }
        self.entries.push(entry);
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
        let ledger = Self::default();
        {
            let mut state = ledger.state.write().expect("fresh lock");
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
}
