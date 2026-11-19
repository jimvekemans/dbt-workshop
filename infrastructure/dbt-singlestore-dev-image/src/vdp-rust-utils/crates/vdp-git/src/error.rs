use thiserror::Error;

/// Why a git operation failed.
#[derive(Debug, Error)]
pub enum GitError {
    #[error("cannot run git")]
    Spawn(#[from] std::io::Error),

    #[error("`git {command}` failed: {detail}")]
    Command { command: String, detail: String },

    #[error("unexpected git output: {0}")]
    Parse(String),

    /// A write the [`crate::WriteGuard`] forbids (e.g. anything on `dev`).
    #[error("refused: {0}")]
    Guard(String),

    /// The remote refused a push; `rejection` says why in a way the UI can act on.
    #[error("push of {branch} rejected ({rejection:?}): {detail}")]
    Push {
        branch: String,
        rejection: crate::PushRejection,
        detail: String,
    },

    #[error("`git {command}` did not finish within {seconds} s and was stopped")]
    Timeout { command: String, seconds: u64 },
}
