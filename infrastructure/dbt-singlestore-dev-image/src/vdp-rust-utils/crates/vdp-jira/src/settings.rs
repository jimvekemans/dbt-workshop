use serde::{Deserialize, Serialize};

/// `[jira]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct JiraSettings {
    pub base_url: String,
    /// Secrets holding the account and its API token (never prompted for).
    pub user_secret: String,
    pub token_secret: String,
    /// Fix versions that always take a ticket out of a release.
    pub excluded_fix_versions: Vec<String>,
    /// Transition `vdp jira advance` takes, by name. Empty: the first one Jira offers (the Python behaviour).
    pub advance_transition: String,
    /// Custom field holding the sprints.
    pub sprint_field: String,
}

impl Default for JiraSettings {
    fn default() -> Self {
        Self {
            base_url: "https://issuetracker.example.com".into(),
            user_secret: "JIRA_USER".into(),
            token_secret: "JIRA_TOKEN".into(),
            excluded_fix_versions: vec!["Nog in te plannen".into()],
            advance_transition: String::new(),
            sprint_field: "customfield_11221".into(),
        }
    }
}

impl JiraSettings {
    /// The name of the config section these settings are read from (`[jira]`).
    pub const SECTION: &'static str = "jira";
}
