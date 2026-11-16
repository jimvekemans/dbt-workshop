use serde::{Deserialize, Serialize};

/// `[release_hub]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HubSettings {
    /// The clone the hub analyses and releases from (a volume in the container).
    pub repo_dir: String,
    /// The repository the clone action proposes.
    pub repo_url: String,
    pub bind: String,
    pub port: u16,
    /// `Access-Control-Allow-Origin` for a frontend on another origin (`npm run dev`). Empty: same origin only.
    pub cors_origin: String,
    /// The built frontend to serve next to the API. Empty: API only (nginx serves the frontend).
    pub ui_dir: String,
    /// Run `[release].validate` (dbt) in the clone before a release is pushed.
    pub validate: bool,
    /// Finished jobs kept in memory.
    pub keep_jobs: usize,
    /// A git command running longer is stopped (a hung network or ssh prompt would block every job).
    pub git_timeout_secs: u64,
    /// A conflict without a decision for this long aborts the run (nothing is pushed).
    pub conflict_timeout_secs: u64,
    /// Analyses and job history survive restarts here. Empty: in memory only.
    pub state_dir: String,
    /// `gitlab`: release managers sign in with a personal GitLab token; only those who may force-push main
    /// and the release branches may release, and pushes go over HTTPS as them. `none`: no sign-in (local use).
    pub auth: String,
    /// GitLab's address. Empty: from `repo_url` (`git@git.example.com:…` → `https://git.example.com`).
    pub gitlab_url: String,
    /// The project path. Empty: from `repo_url` (`vdp/dbt-vdp`).
    pub gitlab_project: String,
    /// Where pushes go. Empty: the project's HTTPS URL.
    pub push_url: String,
    /// The VaultSpeed framework repository, steered from the same UI.
    pub vaultspeed: VaultspeedHubSettings,
}

/// `[release_hub.vaultspeed]`: the vaultspeed-automation-framework repository (its own clone and GitLab project).
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

impl Default for HubSettings {
    fn default() -> Self {
        Self {
            repo_dir: "/opt/project".into(),
            repo_url: "git@git.example.com:vdp/dbt-vdp.git".into(),
            bind: "0.0.0.0".into(),
            port: 8765,
            cors_origin: String::new(),
            ui_dir: String::new(),
            validate: false,
            keep_jobs: 50,
            git_timeout_secs: 600,
            conflict_timeout_secs: 7200,
            state_dir: "/var/lib/vdp-hub".into(),
            auth: "gitlab".into(),
            gitlab_url: String::new(),
            gitlab_project: String::new(),
            push_url: String::new(),
            vaultspeed: VaultspeedHubSettings::default(),
        }
    }
}

impl HubSettings {
    pub const SECTION: &'static str = "release_hub";
}
