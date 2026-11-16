use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid configuration in {origin}: {detail}")]
    Config { origin: String, detail: String },

    /// `known`: the configured environments (`[environments.<name>]`).
    #[error("unknown environment '{name}' (configured: {}; add [environments.{name}] to define it)", .known.join(", "))]
    UnknownEnvironment { name: String, known: Vec<String> },

    #[error("no dbt project (dbt_project.yml) found in {0} or any parent directory")]
    NoProject(PathBuf),

    #[error(transparent)]
    Service(#[from] crate::ServiceError),
}
