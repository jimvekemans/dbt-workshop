use serde::{Deserialize, Serialize};

/// `[release_hub.vaultspeed]`: the vaultspeed-automation-framework repository (its own clone and GitLab project).
/// The hub reads it as an opaque table (`HubSettings::extension`); only this side knows the keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct VaultspeedHubSettings {
    /// Offer VaultSpeed releases in the UI.
    pub enabled: bool,
    pub repo_dir: String,
    pub repo_url: String,
    /// As for dbt: empty means derived from `repo_url`.
    pub gitlab_url: String,
    pub gitlab_project: String,
    pub push_url: String,
}

impl Default for VaultspeedHubSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            repo_dir: "/opt/vaultspeed".into(),
            repo_url: "git@git.example.com:vdp/vaultspeed-automation-framework.git".into(),
            gitlab_url: String::new(),
            gitlab_project: String::new(),
            push_url: String::new(),
        }
    }
}
