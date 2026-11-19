use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not a readable zip archive: {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("{archive} contains an unsafe entry name `{entry}`")]
    UnsafeEntry { archive: PathBuf, entry: String },

    #[error("{archive} expands to more than {limit} bytes; refusing to read it")]
    TooLarge { archive: PathBuf, limit: u64 },

    #[error("staged release `{file}` has an invalid name: {source}")]
    InvalidGenerationName {
        file: String,
        #[source]
        source: vdp_vaultspeed_domain::DomainError,
    },

    #[error("requested generation(s) not staged: {0}")]
    NotStaged(String),

    #[error("{path}: {source}")]
    Sql {
        path: String,
        #[source]
        source: vdp_vaultspeed_sql::SqlError,
    },

    #[error("no handler supports source {0}")]
    Unsupported(String),
}

impl SourceError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }
}
