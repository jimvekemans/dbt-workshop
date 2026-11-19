//! VaultSpeed releases on the vaultspeed-automation-framework repository.
//!
//! The branch model (the team's, kept by every flow here):
//! - `main` is the neutral skeleton: the framework without DAGs or SQL state. It only ever gets skeleton changes.
//! - Every environment branch (`LDV`, `ABT`, `CBT`, `PRD`) is based on `main` and carries its own deploys on top.
//!   Nothing flows from an environment back to `main`, and no branch is ever based on an environment branch.
//! - VaultSpeed's agent pushes one `release/<n>_<TYPE>` branch per zip; CI merges each into LDV and deploys.
//! - A real release is a `feature/<DROP>` branch from `main` collecting zip branches and PML scripts; it is merged
//!   into ABT, CBT and PRD with the message CI deploys on (ABT may also receive single zip branches).
//!
//! [`status`] shows where every zip is deployed; [`flows`] builds releases, merges them into environments, makes
//! direct fixes and resets branches, each previewed first and executed with a lease; [`preview`] plans what CI
//! would deploy.

pub mod changes;
mod error;
pub mod flows;
mod guard;
pub mod preview;
mod settings;
pub mod status;
pub mod zips;

pub use error::VsError;
pub use guard::EnvironmentCommits;
pub use settings::{VsReleaseSettings, VsRules};
