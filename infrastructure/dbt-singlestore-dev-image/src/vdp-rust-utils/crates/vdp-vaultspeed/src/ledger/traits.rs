use std::fmt::Debug;

use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry};

use crate::ledger::LedgerError;

/// Read side of a ledger. A ledger instance is bound to exactly one environment.
pub trait LedgerReader: Send + Sync + Debug {
    /// The latest successful entry for this change: matched on the raw fingerprint
    /// when known, otherwise (or additionally) on the effective fingerprint, which is
    /// all legacy state recorded.
    fn find_applied(
        &self,
        raw: Option<&Fingerprint>,
        effective: &Fingerprint,
    ) -> Result<Option<LedgerEntry>, LedgerError>;

    /// The latest successful entry recorded under an object key (see `ObjectKey`).
    fn latest_for_object(&self, object_key: &str) -> Result<Option<LedgerEntry>, LedgerError>;

    /// Highest sequence number in use (0 when empty).
    fn last_sequence(&self) -> Result<u64, LedgerError>;

    /// All entries in sequence order.
    fn entries(&self) -> Result<Vec<LedgerEntry>, LedgerError>;
}

/// Write side of a ledger.
pub trait LedgerWriter: Send + Sync + Debug {
    /// Records an entry. The ledger assigns `sequence`; the value passed in is ignored.
    fn record(&self, entry: LedgerEntry) -> Result<LedgerEntry, LedgerError>;
}

/// Append-only mirror of entries that were already sequenced by a primary ledger.
pub trait LedgerSink: Send + Sync + Debug {
    fn append(&self, entry: &LedgerEntry) -> Result<(), LedgerError>;
}

/// A ledger that can be both read and written.
pub trait Ledger: LedgerReader + LedgerWriter {}

impl<T: LedgerReader + LedgerWriter> Ledger for T {}
