use std::path::PathBuf;

use thiserror::Error;

/// Why a source file or the catalog could not be read or written.
#[derive(Debug, Error)]
pub enum SourcesError {
    #[error("{path}: {detail}")]
    Yaml { path: PathBuf, detail: String },

    #[error("{path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("database")]
    Catalog(#[from] vdp_db::DbError),

    #[error("{0}")]
    Invalid(String),
}
