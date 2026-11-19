//! The release hub: the module behind the release UI.
//!
//! - One clone of the dbt repository (`[release_hub].repo_dir`), cloned and synced by jobs.
//! - Analyses ([`vdp_release::Analysis`]) are stored with their selection, so executing and announcing refer to
//!   what the user saw; an analysis whose refs moved is refused (409).
//! - Git work runs as [`jobs`], one at a time, with a step log read by cursor (`GET /jobs/{id}?after=<seq>`).
//! - Credentials stay on the server: Jira and SMTP come from the vdp secrets.
//! - [`http`] serves `/api/v1` with tiny_http and, optionally, the built Svelte frontend.
//! - Further repositories plug in as [`HubExtension`]s below their own URL segment (VaultSpeed releases:
//!   `/api/v1/vaultspeed/...`, in `vdp-vaultspeed-hub`); the hub keeps sign-in, sessions and the status for all.

pub mod api;
pub mod auth;
mod error;
pub mod extension;
pub mod http;
mod hub;
pub mod jobs;
mod settings;
#[cfg(feature = "ts")]
mod ts;

pub use auth::GitLab;
pub use error::HubError;
pub use extension::{ExtensionHost, ExtensionRequest, HubExtension, Need};
pub use http::{HubServer, ServeOptions};
pub use hub::{Actor, Hub, HubConfig, Validator, repo_status_of};
pub use settings::HubSettings;
#[cfg(feature = "ts")]
pub use ts::{declaration, typescript, typescript_with};
