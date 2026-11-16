use thiserror::Error;
use vdp_release::ReleaseError;

/// An API error; each maps to an HTTP status.
#[derive(Debug, Error)]
pub enum HubError {
    /// 400.
    #[error("{0}")]
    BadRequest(String),
    /// 401: sign in first.
    #[error("{0}")]
    Unauthorized(String),
    /// 403: signed in, but not allowed (e.g. no force-push rights).
    #[error("{0}")]
    Forbidden(String),
    /// 404.
    #[error("not found: {0}")]
    NotFound(String),
    /// 409: the request conflicts with the state (no clone yet, a stale analysis).
    #[error("{0}")]
    Conflict(String),
    /// 409: a job is running.
    #[error("busy: {0}")]
    Busy(String),
    /// 500.
    #[error("{0}")]
    Internal(String),
}

impl HubError {
    pub const fn status(&self) -> u16 {
        match self {
            Self::BadRequest(_) => 400,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::Conflict(_) | Self::Busy(_) => 409,
            Self::Internal(_) => 500,
        }
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Busy(_) => "busy",
            Self::Internal(_) => "internal",
        }
    }
}

impl From<ReleaseError> for HubError {
    fn from(error: ReleaseError) -> Self {
        match error {
            ReleaseError::Invalid(m) => Self::BadRequest(m),
            ReleaseError::Stale(m) => Self::Conflict(format!("stale analysis: {m}")),
            other => Self::Internal(other.to_string()),
        }
    }
}

impl From<vdp_git::GitError> for HubError {
    fn from(error: vdp_git::GitError) -> Self {
        Self::Internal(error.to_string())
    }
}
