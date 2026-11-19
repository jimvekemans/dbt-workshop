//! VaultSpeed releases on the vaultspeed-automation-framework repository.
//!
//! A domain orchestrator over the [`vdp_git::Vcs`] port, used by the CLI and by the VaultSpeed side of the release hub
//! (`vdp-vaultspeed-hub`).
//!
//! The branch model (the team's, kept by every flow here):
//! - `main` is the neutral skeleton: the framework without DAGs or SQL state. It only ever gets skeleton changes.
//! - Every environment branch (`LDV`, `ABT`, `CBT`, `PRD`) is based on `main` and carries its own deploys on top. Nothing
//!   flows from an environment back to `main`, and no branch is ever based on an environment branch.
//! - VaultSpeed's agent pushes one `release/<n>_<TYPE>` branch per zip; CI merges each into LDV and deploys.
//! - A real release is a `feature/<DROP>` branch from `main` collecting zip branches and PML scripts; it is merged into
//!   ABT, CBT and PRD with the message CI deploys on (ABT may also receive single zip branches).
//!
//! # Responsibilities
//!
//! - [`status`]: where every zip is deployed, from the remote-tracking refs.
//! - [`flows`]: build releases, merge them into environments, make direct fixes and reset branches, each previewed first
//!   and executed with a lease.
//! - [`preview`]: plan what CI would deploy for what is staged.
//! - [`VsRules`] and [`EnvironmentCommits`]: the branch rules and the guard that keeps environment content where it is.
//! - [`changes`] and [`zips`]: file and zip-entry diffs between two states, and reading a generation zip from git.
//!
//! # Example
//!
//! ```
//! use vdp_vaultspeed_release::{VsReleaseSettings, VsRules};
//!
//! let rules = VsRules::new(VsReleaseSettings::default());
//! assert_eq!(rules.release_branch("2026_DROP25.1")?, "feature/2026_DROP25.1");
//! assert!(rules.release_branch("next week").is_err());
//! assert!(rules.is_environment("PRD"));
//! assert_eq!(rules.lower("ABT"), Some("LDV"));
//! # Ok::<(), vdp_vaultspeed_release::VsError>(())
//! ```

pub mod changes;
mod error;
pub mod flows;
mod guard;
pub mod preview;
mod settings;
/// Where every zip and environment stands, from the remote-tracking refs.
pub mod status;
/// Reading a generation zip from git and collecting its tickets.
pub mod zips;

pub use error::VsError;
pub use guard::{CommitCheck, EnvironmentCommits};
pub use settings::{VsReleaseSettings, VsRules};
