//! The shared core every VDP module builds on: environments, configuration, the service context and batch reporting.
//!
//! A building block: it has no dependency on any other `vdp-*` crate, and modules never name each other's concrete
//! types. They receive what they need through a [`Context`] that the composition root (`vdp-cli`) fills.
//!
//! # Responsibilities
//!
//! - [`Environment`]: the built-in deployment environments (LDV, ABT, CBT, PRD), with the aliases `dev`, `val` and `prd`
//!   for LDV, ABT and PRD.
//! - [`Config`]: built-in defaults, overlaid by `/etc/vdp/vdp.toml`, the project's `vdp.toml` and `$VDP_CONFIG`. The core
//!   owns `[core]`, `[secrets]` and `[environments.*]`; each module reads its own section with [`Config::section`].
//! - [`BatchReport`]: what happened to every item of a batch (succeeded, failed or skipped) and the exit [`Outcome`] it
//!   maps to.
//! - [`milestone`]: structured lifecycle [`Milestone`] events (`initialized`, `attached`, `skipped`, `preview`,
//!   `completed`).
//! - [`present`]: plain-text [`present::Table`] and 88-column [`present::Panel`] formatting.
//! - [`ProjectLayout`]: the dbt project (the directory holding `dbt_project.yml`) and its git repository.
//! - [`Context`]: what a module receives. Services (secrets, a database session, the dbt engine, ...) are registered once
//!   by the composition root and created lazily on first use.
//!
//! # Example
//!
//! ```
//! use vdp_core::{BatchReport, Config, Context, Environment, Outcome};
//!
//! // Built-in defaults only: no configuration file is read.
//! let config = Config::load(&[])?;
//! assert_eq!(config.environment_named("prd")?, Environment::PRD);
//! // `dev` is an alias of LDV, whose short name it is.
//! assert_eq!(config.environment_named("dev")?, Environment::LDV);
//! assert_eq!(Environment::LDV.short_name(), "dev");
//!
//! // A service is registered once and created on first use.
//! let ctx = Context::builder(config).provide_value(42_u32).build();
//! assert_eq!(ctx.service::<u32>()?, 42);
//!
//! // What happened to every item of a batch, and the exit code it maps to.
//! let mut report: BatchReport<&str, u32, String> = BatchReport::with_capacity(2);
//! report.push_succeeded("a", 1);
//! report.push_failed("b", "boom".to_owned());
//! assert_eq!(report.outcome(), Outcome::Failures);
//! assert_eq!(report.outcome().exit_code(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod batch;
mod config;
mod context;
mod environment;
mod error;
mod file_status;
pub mod milestone;
mod outcome;
pub mod present;
mod project;

pub use batch::{BatchCounts, BatchOutcome, BatchRecord, BatchReport};
pub use config::{Config, CoreSettings, EnvironmentSettings, SecretsSettings, deep_merge};
pub use context::{Context, ContextBuilder, ServiceError};
pub use environment::Environment;
pub use error::{CoreError, display_chain};
pub use file_status::FileStatus;
pub use milestone::Milestone;
pub use outcome::Outcome;
pub use project::ProjectLayout;
