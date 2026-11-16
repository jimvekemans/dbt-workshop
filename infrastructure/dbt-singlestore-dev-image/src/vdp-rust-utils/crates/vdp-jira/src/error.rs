use thiserror::Error;

#[derive(Debug, Error)]
pub enum JiraError {
    #[error("Jira request {url} failed: {detail}")]
    Request { url: String, detail: String },

    #[error("Jira response for {url}: {detail}")]
    Response { url: String, detail: String },

    #[error("{0}")]
    Invalid(String),
}
