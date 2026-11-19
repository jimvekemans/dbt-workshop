use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid FMC sources file {path}: {source}")]
    FmcSources {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid configuration: {0}")]
    Invalid(String),
}
