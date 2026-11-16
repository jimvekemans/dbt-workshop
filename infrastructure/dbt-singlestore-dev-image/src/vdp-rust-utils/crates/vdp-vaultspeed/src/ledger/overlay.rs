use std::sync::Arc;

use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry};

use crate::ledger::{LedgerError, LedgerReader, LedgerWriter, MemoryLedger};

/// Reads through to a base ledger, buffers all writes in memory.
///
/// Preview runs use it so that later sources of the same run see what earlier sources
/// *would* have applied, without persisting anything.
#[derive(Debug)]
pub struct OverlayLedger {
    base: Arc<dyn LedgerReader>,
    overlay: MemoryLedger,
}

impl OverlayLedger {
    pub fn new(base: Arc<dyn LedgerReader>) -> Result<Self, LedgerError> {
        let offset = base.last_sequence()?;
        Ok(Self {
            base,
            overlay: MemoryLedger::starting_after(offset),
        })
    }

    /// Entries written to the overlay only.
    pub fn pending(&self) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.overlay.entries()
    }
}

impl LedgerReader for OverlayLedger {
    fn find_applied(
        &self,
        raw: Option<&Fingerprint>,
        effective: &Fingerprint,
    ) -> Result<Option<LedgerEntry>, LedgerError> {
        match self.overlay.find_applied(raw, effective)? {
            Some(entry) => Ok(Some(entry)),
            None => self.base.find_applied(raw, effective),
        }
    }

    fn latest_for_object(&self, object_key: &str) -> Result<Option<LedgerEntry>, LedgerError> {
        match self.overlay.latest_for_object(object_key)? {
            Some(entry) => Ok(Some(entry)),
            None => self.base.latest_for_object(object_key),
        }
    }

    fn last_sequence(&self) -> Result<u64, LedgerError> {
        self.overlay.last_sequence()
    }

    fn entries(&self) -> Result<Vec<LedgerEntry>, LedgerError> {
        let mut entries = self.base.entries()?;
        entries.extend(self.overlay.entries()?);
        Ok(entries)
    }
}

impl LedgerWriter for OverlayLedger {
    fn record(&self, entry: LedgerEntry) -> Result<LedgerEntry, LedgerError> {
        self.overlay.record(entry)
    }
}

#[cfg(test)]
mod tests {
    use vdp_vaultspeed_domain::LedgerOutcome;

    use super::*;
    use crate::ledger::memory::tests::entry;

    #[test]
    fn writes_never_reach_the_base() {
        let base = Arc::new(MemoryLedger::new());
        base.record(entry(Some("a"), "a", LedgerOutcome::Applied))
            .unwrap();
        let overlay = OverlayLedger::new(base.clone()).unwrap();
        let recorded = overlay
            .record(entry(Some("b"), "b", LedgerOutcome::Applied))
            .unwrap();

        assert_eq!(
            recorded.sequence, 2,
            "overlay sequences continue after the base"
        );
        assert!(
            overlay
                .find_applied(Some(&Fingerprint::of("a")), &Fingerprint::of("a"))
                .unwrap()
                .is_some()
        );
        assert!(
            overlay
                .find_applied(Some(&Fingerprint::of("b")), &Fingerprint::of("b"))
                .unwrap()
                .is_some()
        );
        assert!(
            base.find_applied(Some(&Fingerprint::of("b")), &Fingerprint::of("b"))
                .unwrap()
                .is_none()
        );
        assert_eq!(overlay.pending().unwrap().len(), 1);
    }
}
