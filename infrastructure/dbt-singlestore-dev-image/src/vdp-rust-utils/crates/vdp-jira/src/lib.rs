//! Jira for the release flow.
//!
//! - [`FixVersion`]: `2026_DROP05` / `2026_EDROP01`, ordered like the Python string comparison (year, then
//!   DROP before EDROP) but with numeric drop numbers, so `DROP10` comes after `DROP9`.
//! - [`Ticket`] keeps every raw fix version name, so exclusions such as "Nog in te plannen" work (the Python
//!   code dropped every name that was not a DROP before checking them).
//! - [`IssueTracker`]: the port the release flow uses; [`JiraClient`] is the REST adapter.

mod client;
mod error;
mod fix_version;
mod settings;

use std::fmt::Debug;

pub use client::JiraClient;
pub use error::JiraError;
pub use fix_version::FixVersion;
pub use settings::JiraSettings;

/// The ticket fields the toolkit uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ticket {
    pub key: String,
    pub summary: String,
    pub status: String,
    /// Every fix version name, as in Jira.
    pub fix_versions: Vec<String>,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    pub components: Vec<String>,
    /// The active sprint, else the first one listed.
    pub sprint: Option<String>,
}

impl Ticket {
    /// The latest regular DROP fix version (what the Python code called `fix_version`).
    pub fn release(&self) -> Option<FixVersion> {
        self.drop_versions().filter(|v| !v.emergency).max()
    }

    /// The emergency drops (`<year>_EDROP<n>`) the ticket targets.
    pub fn emergency_drops(&self) -> Vec<FixVersion> {
        self.drop_versions().filter(|v| v.emergency).collect()
    }

    fn drop_versions(&self) -> impl Iterator<Item = FixVersion> + '_ {
        self.fix_versions
            .iter()
            .filter_map(|name| name.parse().ok())
    }
}

/// `https://<jira>/browse/<KEY>`.
pub fn browse_url(base_url: &str, key: &str) -> String {
    format!("{}/browse/{key}", base_url.trim_end_matches('/'))
}

/// A status change a ticket can make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub id: String,
    pub name: String,
    /// The status it leads to.
    pub to: String,
}

pub trait IssueTracker: Send + Sync + Debug {
    fn ticket(&self, key: &str) -> Result<Ticket, JiraError>;
    fn transitions(&self, key: &str) -> Result<Vec<Transition>, JiraError>;
    fn transition(&self, key: &str, transition_id: &str) -> Result<(), JiraError>;
}
