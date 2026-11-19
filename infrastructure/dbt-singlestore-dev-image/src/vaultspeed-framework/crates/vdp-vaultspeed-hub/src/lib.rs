//! The VaultSpeed side of the release hub: a clone of the vaultspeed-automation-framework repository with its
//! own jobs, plugged into `vdp-release-hub` as a [`HubExtension`](vdp_release_hub::HubExtension).
//!
//! The flows are `vdp_vaultspeed_release`'s (the CLI runs the same code); [`VsHub`] adds the clone's lifecycle,
//! runs flows as jobs and serves files and history for the editors, and `extension` maps `/api/v1/vaultspeed/...`
//! onto it. Register `Arc::new(VaultspeedExtension::new(..))` with the hub to switch it on.

pub mod api;
mod extension;
mod hub;
mod settings;
#[cfg(feature = "ts")]
mod ts;

pub use extension::{SEGMENT, VaultspeedExtension};
pub use hub::{VsConfig, VsHub};
pub use settings::VaultspeedHubSettings;
#[cfg(feature = "ts")]
pub use ts::declarations;
