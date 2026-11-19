use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("ledger file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("ledger file {path}, line {line}: {source}")]
    Corrupt {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },

    #[error("ledger table: {0}")]
    Database(#[from] vdp_db::DbError),

    #[error("ledger table row is malformed: {0}")]
    MalformedRow(String),

    #[error("cannot parse legacy file {path}: {source}")]
    Legacy {
        path: PathBuf,
        #[source]
        source: vdp_vaultspeed_sql::SqlError,
    },

    #[error("ledger lock poisoned")]
    Poisoned,
}
