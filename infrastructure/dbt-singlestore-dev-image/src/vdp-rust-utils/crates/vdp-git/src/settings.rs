//! The `[git]` configuration section.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// `[git]`: how the command line runs the system `git`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GitSettings {
    /// Seconds a single git command may run before it is stopped (a hung network call or an ssh prompt would block the
    /// command for ever). `0`: no limit.
    pub timeout_secs: u64,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self { timeout_secs: 600 }
    }
}

impl GitSettings {
    /// The name of the config section these settings are read from (`[git]`).
    pub const SECTION: &'static str = "git";

    /// The limit for [`GitCli::with_timeout`](crate::GitCli::with_timeout): `None` when `timeout_secs` is `0`.
    #[must_use]
    pub fn timeout(&self) -> Option<Duration> {
        (self.timeout_secs > 0).then(|| Duration::from_secs(self.timeout_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_commands_stop_after_ten_minutes_unless_configured_otherwise() {
        assert_eq!(
            GitSettings::default().timeout(),
            Some(Duration::from_mins(10))
        );
        let short: GitSettings =
            serde_json::from_value(serde_json::json!({"timeout_secs": 5})).unwrap();
        assert_eq!(short.timeout(), Some(Duration::from_secs(5)));
        let none: GitSettings =
            serde_json::from_value(serde_json::json!({"timeout_secs": 0})).unwrap();
        assert_eq!(none.timeout(), None);
    }

    #[test]
    fn an_unknown_git_key_is_refused() {
        let error =
            serde_json::from_value::<GitSettings>(serde_json::json!({"timeout": 5})).unwrap_err();
        assert!(
            error.to_string().contains("unknown field `timeout`"),
            "{error}"
        );
    }
}
