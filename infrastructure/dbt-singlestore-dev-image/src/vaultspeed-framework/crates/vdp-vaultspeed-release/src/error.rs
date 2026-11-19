use thiserror::Error;

/// Why a release flow could not run.
#[derive(Debug, Error)]
pub enum VsError {
    #[error(transparent)]
    Git(#[from] vdp_git::GitError),

    /// A rule refused the action (environment content leaving its environment, a skeleton path on main, ...).
    #[error("{0}")]
    Refused(String),

    /// Origin moved since the preview.
    #[error("{0}")]
    Stale(String),

    #[error("{0}")]
    Invalid(String),

    /// The release manager stopped the run at a conflict.
    #[error("aborted: {0}")]
    Aborted(String),

    #[error("zip {name}: {detail}")]
    Zip { name: String, detail: String },

    #[error("plan preview: {0}")]
    Plan(String),
}
