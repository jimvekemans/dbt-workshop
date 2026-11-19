//! The VaultSpeed side of the release hub: a clone of the vaultspeed-automation-framework repository with its own jobs,
//! plugged into `vdp-release-hub` as a [`HubExtension`](vdp_release_hub::HubExtension).
//!
//! A domain orchestrator and an adapter. The flows are `vdp_vaultspeed_release`'s (the CLI runs the same code); this crate
//! adds the hub around them. `vdp-vaultspeed-cli` registers it with the hub.
//!
//! # Responsibilities
//!
//! - [`VsHub`]: the clone's lifecycle, the release flows run as jobs, files and history served to the editors.
//! - `extension`: maps `/api/v1/vaultspeed/...` onto it ([`VaultspeedExtension`]).
//! - [`api`]: the request and response types of those routes.
//! - [`VaultspeedHubSettings`] reads the `[release_hub.vaultspeed]` settings.
//!
//! Register `Arc::new(VaultspeedExtension::new(..))` with the hub to switch it on.
//!
//! # Example
//!
//! ```
//! use std::time::Duration;
//! use vdp_release_hub::HubExtension;
//! use vdp_vaultspeed::VaultspeedSettings;
//! use vdp_vaultspeed_hub::{SEGMENT, VaultspeedExtension, VsConfig};
//! use vdp_vaultspeed_release::VsReleaseSettings;
//!
//! let config = VsConfig {
//!     repo_dir: "/srv/vaultspeed".into(),
//!     repo_url: "https://git.example.com/vdp/vaultspeed.git".into(),
//!     gitlab: None,
//!     release: VsReleaseSettings::default(),
//!     vaultspeed: VaultspeedSettings::default(),
//!     keep_jobs: 50,
//!     git_timeout: None,
//!     conflict_timeout: Duration::from_secs(30 * 60),
//!     state_dir: None,
//! };
//! let extension = VaultspeedExtension::new(config, None);
//! assert_eq!(extension.segment(), SEGMENT);
//! ```

pub mod api;
mod extension;
mod hub;
mod settings;
#[cfg(feature = "ts")]
mod ts;

pub use extension::{SEGMENT, VaultspeedExtension};
pub use hub::{ChangeFileRequest, VsConfig, VsHub};
pub use settings::VaultspeedHubSettings;
#[cfg(feature = "ts")]
pub use ts::declarations;
