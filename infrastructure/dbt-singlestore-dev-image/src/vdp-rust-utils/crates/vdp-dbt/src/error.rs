use std::path::PathBuf;

use thiserror::Error;

/// Why dbt could not be located, run or read.
#[derive(Debug, Error)]
pub enum DbtError {
    #[error("no dbt binary found: set VDP_DBT_BIN or [dbt].binary, or put dbt v2 on PATH")]
    NotFound,

    #[error("cannot run {binary}")]
    Spawn {
        binary: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("dbt grammar: {0}")]
    Grammar(String),

    #[error("signal handling: {0}")]
    Signals(String),

    #[error("{path}: {detail}")]
    Project { path: PathBuf, detail: String },

    #[error("{path}: {detail}")]
    Profile { path: PathBuf, detail: String },

    #[error("{path}: {detail}")]
    Artifact { path: PathBuf, detail: String },
}
