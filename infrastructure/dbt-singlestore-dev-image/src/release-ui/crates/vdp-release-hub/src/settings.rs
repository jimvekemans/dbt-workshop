use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// `[release_hub]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
    /// The settings of the extensions, by segment (`[release_hub.vaultspeed]`): only the extension knows what
    /// they mean, see [`HubSettings::extension`].
    #[serde(flatten)]
    pub extensions: BTreeMap<String, serde_json::Value>,
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
            extensions: BTreeMap::new(),
        }
    }
}

impl HubSettings {
    /// The name of the config section these settings are read from (`[release_hub]`).
    pub const SECTION: &'static str = "release_hub";

    /// The settings table of one extension (`[release_hub.<name>]`); its defaults when the table is absent.
    /// Typos in other keys of `[release_hub]` end up here unread, so this refuses a table that is not one.
    ///
    /// # Errors
    ///
    /// A message naming `[release_hub.<name>]` when the table does not deserialize into `T`.
    pub fn extension<T: DeserializeOwned + Default>(&self, name: &str) -> Result<T, String> {
        match self.extensions.get(name) {
            None => Ok(T::default()),
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|e| format!("[{}.{name}]: {e}", Self::SECTION)),
        }
    }
}
