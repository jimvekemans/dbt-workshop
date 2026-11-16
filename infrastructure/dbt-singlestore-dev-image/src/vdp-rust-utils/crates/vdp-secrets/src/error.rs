use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("secret {keys} not found in {provider}")]
    Missing { keys: String, provider: String },

    #[error("cannot read {path}: {detail}")]
    File { path: PathBuf, detail: String },

    #[error("vault request to {url} failed: {detail}")]
    Vault { url: String, detail: String },
}
