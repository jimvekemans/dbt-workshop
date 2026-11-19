use std::path::PathBuf;

use thiserror::Error;

/// Why a ledger could not be read or written.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("ledger file {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("ledger file {path}, line {line}")]
    Corrupt {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },

    #[error("ledger table")]
    Database(#[from] vdp_db::DbError),

    #[error("ledger table row is malformed: {0}")]
    MalformedRow(String),

    #[error("cannot parse legacy file {path}")]
    Legacy {
        path: PathBuf,
        #[source]
        source: vdp_vaultspeed_sql::SqlError,
    },

    #[error("ledger lock poisoned")]
    Poisoned,
}
