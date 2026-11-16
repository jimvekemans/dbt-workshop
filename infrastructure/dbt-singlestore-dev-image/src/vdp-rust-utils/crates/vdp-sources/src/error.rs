use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourcesError {
    #[error("{path}: {detail}")]
    Yaml { path: PathBuf, detail: String },

    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("database: {0}")]
    Catalog(#[from] vdp_db::DbError),

    #[error("{0}")]
    Invalid(String),
}
