use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: {source}")]
    Csv {
        path: PathBuf,
        #[source]
        source: csv::Error,
    },

    #[error("cannot serialise report: {0}")]
    Json(#[from] serde_json::Error),
}

pub(crate) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> ReportError {
    let path = path.to_owned();
    move |source| ReportError::Io { path, source }
}
