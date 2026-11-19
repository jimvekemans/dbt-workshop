use std::fmt::Debug;

use thiserror::Error;
use vdp_vaultspeed_domain::{Evidence, ObjectRef, Statement};

/// A failure of the catalog probe, with its message.
#[derive(Debug, Error)]
#[error("catalog probe failed: {0}")]
pub struct ProbeError(pub String);

/// What the live catalog says about a statement's effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// The effect already holds; running the statement would be redundant.
    Holds(Evidence),
    /// The catalog is in a state the statement cannot reconcile, e.g. the column it adds
    /// exists with a different type. Running would fail, skipping would hide drift.
    Contradicts(String),
    /// Not decidable from the catalog (or not this probe's business): run it.
    Unknown,
}

/// Inspects the live catalog to prove that a statement's effect already holds.
///
/// Implementations answer only for statement kinds where "the catalog looks right"
/// really means "nothing to do" (e.g. `ADD COLUMN` when the column exists with the same
/// type) and return [`ProbeVerdict::Unknown`] otherwise.
pub trait CatalogProbe: Send + Sync + Debug {
    /// What the catalog says about `statement`.
    ///
    /// # Errors
    ///
    /// [`ProbeError`] when the catalog cannot be queried.
    fn verify(&self, statement: &Statement) -> Result<ProbeVerdict, ProbeError>;

    /// Whether a table or view exists in the catalog: `Some(false)` proves it is gone (renamed or dropped
    /// outside the framework although the ledger says it was created). `None`: not known, which is the
    /// answer of probes that cannot tell and of objects without a database name.
    ///
    /// # Errors
    ///
    /// [`ProbeError`] when the catalog cannot be queried.
    fn exists(&self, _object: &ObjectRef) -> Result<Option<bool>, ProbeError> {
        Ok(None)
    }
}
