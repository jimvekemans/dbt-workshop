//! The release hub: the module behind the release UI.
//!
//! - One clone of the dbt repository (`[release_hub].repo_dir`), cloned and synced by jobs.
//! - Analyses ([`vdp_release::Analysis`]) are stored with their selection, so executing and announcing refer to
//!   what the user saw; an analysis whose refs moved is refused (409).
//! - Git work runs as [`jobs`], one at a time, with a step log read by cursor (`GET /jobs/{id}?after=<seq>`).
//! - Credentials stay on the server: Jira and SMTP come from the vdp secrets.
//! - [`http`] serves `/api/v1` with tiny_http and, optionally, the built Svelte frontend.
//! - VaultSpeed releases (`/api/v1/vaultspeed/...`): a second clone, of the framework repository, with its own
//!   jobs; see `vdp_vaultspeed_release` for the branch model and the guards.

pub mod api;
pub mod auth;
mod error;
pub mod http;
mod hub;
pub mod jobs;
mod settings;
#[cfg(feature = "ts")]
mod ts;
mod vaultspeed;

pub use auth::GitLab;
pub use error::HubError;
pub use http::{HubServer, ServeOptions};
pub use hub::{Actor, Hub, HubConfig, Validator};
pub use settings::{HubSettings, VaultspeedHubSettings};
#[cfg(feature = "ts")]
pub use ts::typescript;
pub use vaultspeed::{VsConfig, VsHub};
