use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error(transparent)]
    Git(#[from] vdp_git::GitError),

    #[error("Jira: {0}")]
    Jira(String),

    #[error("{0}")]
    Refused(String),

    #[error(
        "{action} failed: {detail}. Resolve it by hand (the branch is checked out), then re-run"
    )]
    Apply { action: String, detail: String },

    #[error("validation failed; the branch was not pushed: {0}")]
    Validation(String),

    #[error("stale analysis: {0}")]
    Stale(String),

    #[error("{0}")]
    Invalid(String),

    /// The release manager stopped the run at a conflict.
    #[error("aborted: {0}")]
    Aborted(String),
}
