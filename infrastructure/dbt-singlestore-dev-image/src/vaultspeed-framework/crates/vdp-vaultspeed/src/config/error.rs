use std::path::PathBuf;

use thiserror::Error;

/// Why the VaultSpeed configuration or its source list could not be loaded.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid FMC sources file {path}")]
    FmcSources {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid configuration: {0}")]
    Invalid(String),
}
