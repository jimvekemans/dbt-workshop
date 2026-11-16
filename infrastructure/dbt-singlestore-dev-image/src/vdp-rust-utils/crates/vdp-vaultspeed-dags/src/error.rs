use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DagError {
    #[error("tree-sitter could not load the Python grammar: {0}")]
    Grammar(String),

    #[error("{dag}: could not be parsed as Python")]
    Unparseable { dag: String },

    #[error("{dag}: transformer `{transformer}` produced overlapping edits")]
    OverlappingEdits { dag: String, transformer: String },

    #[error("{dag}: transformer `{transformer}` cannot apply: {reason}")]
    NotApplicable {
        dag: String,
        transformer: String,
        reason: String,
    },

    #[error("{dag}: result is not valid Python ({detail})")]
    Invalid { dag: String, detail: String },

    #[error("cannot determine the source system of {dag}; add it to `fmc_sources`")]
    UnknownSource { dag: String },

    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Upload(#[from] vdp_ssh::SshError),
}
