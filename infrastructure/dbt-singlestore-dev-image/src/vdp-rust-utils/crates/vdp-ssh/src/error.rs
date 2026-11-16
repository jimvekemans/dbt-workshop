use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SshError {
    #[error("cannot start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("upload to {target} failed: {detail}")]
    Upload { target: String, detail: String },

    #[error("missing secret: {0}")]
    MissingSecret(String),

    #[error("remote command on {target} exited with {status}: {detail}")]
    Remote {
        target: String,
        status: String,
        detail: String,
    },
}
