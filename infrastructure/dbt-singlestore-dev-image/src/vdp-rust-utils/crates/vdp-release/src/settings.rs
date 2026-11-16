use serde::{Deserialize, Serialize};

/// `[release]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ReleaseSettings {
    /// Jira project key in commit messages (`INTEL-1234`).
    pub ticket_prefix: String,
    /// Source of releases (`dev`).
    pub from: String,
    /// Where releases go (`main`).
    pub to: String,
    /// Prefix of release branches.
    pub branch_prefix: String,
    /// dbt arguments that validate the branch before it is pushed.
    pub validate: Vec<String>,
}

impl Default for ReleaseSettings {
    fn default() -> Self {
        Self {
            ticket_prefix: "INTEL".into(),
            from: "origin/dev".into(),
            to: "origin/main".into(),
            branch_prefix: "release/".into(),
            validate: vec!["compile".into(), "--no-partial-parse".into()],
        }
    }
}

impl ReleaseSettings {
    pub const SECTION: &'static str = "release";
}
