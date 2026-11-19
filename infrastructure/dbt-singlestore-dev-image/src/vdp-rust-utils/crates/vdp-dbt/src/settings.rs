use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// `[dbt]`: the engine and the wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DbtSettings {
    /// The dbt v2 binary. Empty: `$VDP_DBT_BIN`, else the first `dbt` on `PATH` that is not this wrapper.
    pub binary: PathBuf,
    /// `engine`: read the flag grammar from the installed dbt (cached per binary); `embedded`: the built-in copy.
    pub grammar: String,
    /// Commands refused without a selector (unless a service user runs them).
    pub guarded_commands: Vec<String>,
    /// Flags (long names) that count as a selection.
    pub selector_flags: Vec<String>,
    /// Secret holding the SingleStore user.
    pub user_secret: String,
    /// A service user ends with this...
    pub service_user_suffix: String,
    /// ...and does not start with this (personal `pu...` accounts).
    pub personal_user_prefix: String,
    /// Commands (and the `--help`/`--version` flags) that run without resolving secrets.
    pub offline_commands: Vec<String>,
    /// After a failed run, print `vdp sources add ...` for sources dbt reports as undefined (dbt1048).
    pub missing_source_hint: bool,
    /// Variables set for dbt unless already set.
    pub env: BTreeMap<String, String>,
}

impl Default for DbtSettings {
    fn default() -> Self {
        Self {
            binary: PathBuf::new(),
            grammar: "engine".into(),
            guarded_commands: vec!["build".into(), "run".into()],
            selector_flags: vec!["select".into(), "selector".into()],
            user_secret: "SINGLESTORE_USER".into(),
            service_user_suffix: "_dbt".into(),
            personal_user_prefix: "pu".into(),
            offline_commands: vec![
                "help".into(),
                "completions".into(),
                "man".into(),
                "init".into(),
            ],
            missing_source_hint: true,
            env: BTreeMap::from([("DBT_ALLOW_EXPERIMENTAL_ADAPTERS".into(), "true".into())]),
        }
    }
}

impl DbtSettings {
    /// The name of the config section these settings are read from (`[dbt]`).
    pub const SECTION: &'static str = "dbt";

    /// Whether `user` is a service user: it ends with `service_user_suffix` and does not start with `personal_user_prefix`.
    pub fn is_service_user(&self, user: &str) -> bool {
        user.ends_with(&self.service_user_suffix) && !user.starts_with(&self.personal_user_prefix)
    }
}
