//! The ledger answers "has this change already been applied in this environment?".
//!
//! Reading and writing are separate traits ([`LedgerReader`], [`LedgerWriter`]) so that
//! planners can only read and audit mirrors ([`LedgerSink`]) can only append.
//! Implementations:
//!
//! - [`MemoryLedger`]: indexes; the building block of all others and the test double.
//! - [`JsonlLedger`]: an append-only file, committed to git as an audit trail.
//! - [`SqlLedger`]: a table inside the environment's own database (source of truth).
//! - [`CompositeLedger`]: reads one ledger, writes to it and to mirrors.
//! - [`OverlayLedger`]: buffers writes in memory on top of a read-only base (preview).
//! - [`LegacyImporter`]: seeds a ledger from the Python framework's `deployed/` files.

mod composite;
mod error;
mod import;
mod jsonl;
mod memory;
mod overlay;
mod sql;
mod traits;

pub use composite::CompositeLedger;
pub use error::LedgerError;
pub use import::{ImportStats, LegacyImporter};
pub use jsonl::JsonlLedger;
pub use memory::MemoryLedger;
pub use overlay::OverlayLedger;
pub use sql::{LedgerTable, SqlLedger};
pub use traits::{Ledger, LedgerReader, LedgerSink, LedgerWriter};
