//! The release hub: the module behind the release UI.
//!
//! A domain orchestrator and a server: it serves the release flows of `vdp-release` over HTTP, with a clone, jobs and
//! sign-in around them. `vdp-release-ui release serve` (the `vdp-release-ui-cli` crate) starts it; further repositories plug in as extensions.
//!
//! # Responsibilities
//!
//! - One clone of the dbt repository (`[release_hub].repo_dir`), cloned and synced by jobs.
//! - Analyses ([`vdp_release::Analysis`]) are stored with their selection, so executing and announcing refer to what the
//!   user saw; an analysis whose refs moved is refused (409).
//! - Git work runs as [`jobs`], one at a time, with a step log read by cursor (`GET /jobs/{id}?after=<seq>`).
//! - Credentials stay on the server: Jira and SMTP come from the vdp secrets.
//! - [`http`] serves `/api/v1` with tiny_http and, optionally, the built Svelte frontend.
//! - Further repositories plug in as [`HubExtension`]s below their own URL segment (VaultSpeed releases:
//!   `/api/v1/vaultspeed/...`, in `vdp-vaultspeed-hub`); the hub keeps sign-in, sessions and the status for all.
//!
//! # Example
//!
//! ```
//! use vdp_release_hub::{GitLab, HubError};
//!
//! // Every API error maps to an HTTP status.
//! assert_eq!(HubError::Conflict("stale analysis".into()).status(), 409);
//!
//! // The GitLab project is derived from the clone URL.
//! let gitlab = GitLab::from_repo_url("https://git.example.com/vdp/dbt-vdp.git").expect("a GitLab URL");
//! assert_eq!(gitlab.project, "vdp/dbt-vdp");
//! ```
//!
//! Serving (binds a port, so it is only compiled here):
//!
//! ```no_run
//! use vdp_release_hub::{Hub, HubServer, ServeOptions};
//!
//! fn serve(hub: std::sync::Arc<Hub>) -> Result<(), vdp_release_hub::HubError> {
//!     let server = HubServer::bind("127.0.0.1:8080", ServeOptions::default())?;
//!     server.run(&hub); // returns when the server stops
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod auth;
pub mod clone_manager;
mod error;
pub mod extension;
pub mod http;
mod hub;
pub mod jobs;
mod settings;
#[cfg(feature = "ts")]
mod ts;

pub use auth::{GitLab, GitLabConfigError, GitLabSource, non_empty};
pub use clone_manager::{CloneConfig, CloneManager, repo_status_of};
pub use error::HubError;
pub use extension::{ActorRequest, ExtensionHost, ExtensionRequest, HubExtension, Need};
pub use http::{HubServer, ServeOptions};
pub use hub::{Actor, Hub, HubConfig, HubServices, Validator};
pub use settings::HubSettings;
#[cfg(feature = "ts")]
pub use ts::{declaration, typescript, typescript_with};
