//! The shared core every VDP module builds on.
//!
//! - [`Environment`]: the built-in deployment environments (LDV, ABT, CBT, PRD).
//! - [`Config`]: built-in defaults, overlaid by `/etc/vdp/vdp.toml`, the project's `vdp.toml` and `$VDP_CONFIG`.
//!   The core owns `[core]`, `[secrets]` and `[environments.*]`; each module reads its own section with
//!   [`Config::section`].
//! - [`ProjectLayout`]: the dbt project (the directory holding `dbt_project.yml`) and its git repository.
//! - [`Context`]: what a module receives. Services (secrets, a database session, the dbt engine, ...) are
//!   registered once by the composition root and created lazily on first use, so a module that only needs git
//!   never logs in to Vault and modules never name each other's concrete types.

mod config;
mod context;
mod environment;
mod error;
mod outcome;
mod project;

pub use config::{Config, CoreSettings, EnvironmentSettings, SecretsSettings, deep_merge};
pub use context::{Context, ContextBuilder, ServiceError};
pub use environment::Environment;
pub use error::CoreError;
pub use outcome::Outcome;
pub use project::ProjectLayout;
