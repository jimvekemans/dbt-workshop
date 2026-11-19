use std::path::PathBuf;

use thiserror::Error;

/// Why SQL text could not be processed.
#[derive(Debug, Error)]
pub enum SqlError {
    #[error("invalid regex in rewrite rule `{rule}`")]
    InvalidRulePattern {
        rule: String,
        #[source]
        source: Box<regex::Error>,
    },

    #[error("rewrite rule file {path} is not valid JSON")]
    InvalidRuleFile {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("cannot read rewrite rules from {path}")]
    RuleIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "client directive `{directive}` is not SQL and cannot be executed; remove it from the script"
    )]
    UnsupportedDirective { directive: String },
}
