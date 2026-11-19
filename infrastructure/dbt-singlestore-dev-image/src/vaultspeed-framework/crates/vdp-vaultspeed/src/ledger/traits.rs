use std::fmt::Debug;

use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry};

use crate::ledger::LedgerError;

/// Read side of a ledger. A ledger instance is bound to exactly one environment.
pub trait LedgerReader: Send + Sync + Debug {
    /// The latest successful entry for this change: matched on the raw fingerprint
    /// when known, otherwise (or additionally) on the effective fingerprint, which is
    /// all legacy state recorded.
    ///
    /// # Errors
    ///
    /// [`LedgerError::Poisoned`] when the ledger's lock was poisoned by a panic. A ledger that reads from a store (a file or a
    /// table) also returns that store's errors.
    fn find_applied(
        &self,
        raw: Option<&Fingerprint>,
        effective: &Fingerprint,
    ) -> Result<Option<LedgerEntry>, LedgerError>;

    /// The latest successful entry recorded under an object key (see `ObjectKey`).
    ///
    /// # Errors
    ///
    /// [`LedgerError::Poisoned`] when the ledger's lock was poisoned by a panic. A ledger that reads from a store (a file or a
    /// table) also returns that store's errors.
    fn latest_for_object(&self, object_key: &str) -> Result<Option<LedgerEntry>, LedgerError>;

    /// Every successful entry indexed under an object key, in sequence order: entries recorded for the
    /// key itself and renames *into* it (a rename is indexed under both its names).
    ///
    /// # Errors
    ///
    /// [`LedgerError::Poisoned`] when the ledger's lock was poisoned by a panic. A ledger that reads from a store (a file or a
    /// table) also returns that store's errors.
    fn object_history(&self, object_key: &str) -> Result<Vec<LedgerEntry>, LedgerError>;

    /// Highest sequence number in use (0 when empty).
    ///
    /// # Errors
    ///
    /// [`LedgerError::Poisoned`] when the ledger's lock was poisoned by a panic. A ledger that reads from a store (a file or a
    /// table) also returns that store's errors.
    fn last_sequence(&self) -> Result<u64, LedgerError>;

    /// All entries in sequence order.
    ///
    /// # Errors
    ///
    /// [`LedgerError::Poisoned`] when the ledger's lock was poisoned by a panic. A ledger that reads from a store (a file or a
    /// table) also returns that store's errors.
    fn entries(&self) -> Result<Vec<LedgerEntry>, LedgerError>;
}

/// Write side of a ledger.
pub trait LedgerWriter: Send + Sync + Debug {
    /// Records an entry. The ledger assigns `sequence`; the value passed in is ignored.
    ///
    /// # Errors
    ///
    /// [`LedgerError::Io`] or [`LedgerError::Database`] when the entry cannot be stored; [`LedgerError::Poisoned`] when the
    /// ledger's lock was poisoned by a panic.
    fn record(&self, entry: LedgerEntry) -> Result<LedgerEntry, LedgerError>;
}

/// Append-only mirror of entries that were already sequenced by a primary ledger.
pub trait LedgerSink: Send + Sync + Debug {
    /// Appends `entry`, which already has its sequence.
    ///
    /// # Errors
    ///
    /// As [`LedgerWriter::record`].
    fn append(&self, entry: &LedgerEntry) -> Result<(), LedgerError>;
}

/// A ledger that can be both read and written.
pub trait Ledger: LedgerReader + LedgerWriter {}

impl<T: LedgerReader + LedgerWriter> Ledger for T {}
