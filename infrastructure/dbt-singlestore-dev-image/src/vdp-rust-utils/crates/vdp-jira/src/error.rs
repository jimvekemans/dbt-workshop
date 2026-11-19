use thiserror::Error;

/// Why a Jira request failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JiraError {
    #[error("Jira authentication failed for {url} (HTTP {status}): {detail}")]
    Auth {
        url: String,
        status: u16,
        detail: String,
    },

    #[error("Jira unreachable at {url}: {detail}")]
    Transport { url: String, detail: String },

    #[error("Jira request {url} failed: {detail}")]
    Request { url: String, detail: String },

    #[error("Jira response for {url}: {detail}")]
    Response { url: String, detail: String },

    #[error("{0}")]
    Invalid(String),
}

impl JiraError {
    /// Setup/infrastructure errors that must stop a batch immediately rather than continuing per ticket.
    pub const fn is_fatal(&self) -> bool {
        matches!(self, Self::Auth { .. } | Self::Transport { .. })
    }
}
