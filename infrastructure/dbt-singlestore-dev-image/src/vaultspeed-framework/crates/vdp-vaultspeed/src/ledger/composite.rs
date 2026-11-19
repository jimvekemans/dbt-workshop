use std::sync::Arc;

use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry};

use crate::ledger::{Ledger, LedgerError, LedgerReader, LedgerSink, LedgerWriter};

/// Reads from and sequences in a primary ledger, then mirrors each entry to sinks
/// (e.g. the `SingleStore` table as primary and the git-tracked JSONL file as mirror).
#[derive(Debug)]
pub struct CompositeLedger {
    primary: Arc<dyn Ledger>,
    mirrors: Vec<Arc<dyn LedgerSink>>,
}

impl CompositeLedger {
    /// Creates a ledger that reads and sequences in `primary` and mirrors every recorded entry to `mirrors`.
    pub fn new(primary: Arc<dyn Ledger>, mirrors: Vec<Arc<dyn LedgerSink>>) -> Self {
        Self { primary, mirrors }
    }
}

impl LedgerReader for CompositeLedger {
    fn find_applied(
        &self,
        raw: Option<&Fingerprint>,
        effective: &Fingerprint,
    ) -> Result<Option<LedgerEntry>, LedgerError> {
        self.primary.find_applied(raw, effective)
    }

    fn latest_for_object(&self, object_key: &str) -> Result<Option<LedgerEntry>, LedgerError> {
        self.primary.latest_for_object(object_key)
    }

    fn object_history(&self, object_key: &str) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.primary.object_history(object_key)
    }

    fn last_sequence(&self) -> Result<u64, LedgerError> {
        self.primary.last_sequence()
    }

    fn entries(&self) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.primary.entries()
    }
}

impl LedgerWriter for CompositeLedger {
    fn record(&self, entry: LedgerEntry) -> Result<LedgerEntry, LedgerError> {
        let recorded = self.primary.record(entry)?;
        for mirror in &self.mirrors {
            // The primary is authoritative: a failing mirror is logged, not fatal.
            if let Err(error) = mirror.append(&recorded) {
                tracing::warn!(error = %vdp_core::display_chain(&error), sequence = recorded.sequence, "ledger mirror append failed");
            }
        }
        Ok(recorded)
    }
}

#[cfg(test)]
mod tests {
    use vdp_vaultspeed_domain::LedgerOutcome;

    use super::*;
    use crate::ledger::MemoryLedger;
    use crate::ledger::memory::tests::entry;

    #[test]
    fn mirrors_receive_sequenced_entries() {
        let primary = Arc::new(MemoryLedger::new());
        let mirror = Arc::new(MemoryLedger::new());
        let ledger = CompositeLedger::new(primary.clone(), vec![mirror.clone()]);
        ledger
            .record(entry(Some("a"), "a", LedgerOutcome::Applied))
            .unwrap();
        ledger
            .record(entry(Some("b"), "b", LedgerOutcome::Applied))
            .unwrap();
        let mirrored = mirror.entries().unwrap();
        assert_eq!(
            mirrored.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(primary.entries().unwrap(), mirrored);
    }
}
