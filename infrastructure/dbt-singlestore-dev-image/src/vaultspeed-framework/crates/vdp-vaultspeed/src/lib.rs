//! VaultSpeed deployments: what the VaultSpeed Automation Framework did as the separate `vsaf` workspace, now a vdp module
//! (`vdp vaultspeed …`).
//!
//! The domain orchestrator of the framework. It deploys staged **PML scripts** and **VaultSpeed generations** (DDL, ETL,
//! COMP, FMC) to SingleStore and Airflow and knows, per environment, exactly what already ran (the ledger). The modules
//! keep the boundaries the separate crates had: planners read the ledger only through [`ledger::LedgerReader`], the engine
//! depends on traits only, and [`wiring`] is the one place that picks implementations (the CLI and the release hub both
//! use it, so they plan the same way).
//!
//! # Responsibilities
//!
//! - [`config`]: the `[vaultspeed]` settings.
//! - [`sources`]: find, read and interpret what is staged, and tidy it afterwards.
//! - [`planner`]: decide per statement group whether it runs, from the ledger and the catalog.
//! - [`executor`] and [`engine`]: run a plan, record every outcome, and render and publish DAGs.
//! - [`ledger`] and [`history`]: remember what ran (the source of truth) and audit every statement.
//! - [`report`]: render a run for people and machines.
//! - [`wiring`]: compose all of it from the settings.
//!
//! # Example
//!
//! ```
//! use vdp_vaultspeed::VaultspeedSettings;
//! use vdp_vaultspeed::ledger::{LedgerReader, MemoryLedger};
//! use vdp_vaultspeed::sources::StagingLayout;
//!
//! let settings = VaultspeedSettings::default();
//! settings.validate()?;
//!
//! let layout = StagingLayout::new(&settings.paths.vaultspeed_dir);
//! assert!(layout.release_dir().ends_with("staging/release"));
//! assert!(layout.pml_dir().ends_with("staging/pre_model_input"));
//!
//! // Nothing has run yet.
//! assert_eq!(MemoryLedger::new().last_sequence()?, 0);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod config;
pub mod engine;
pub mod executor;
pub mod history;
pub mod ledger;
pub mod planner;
pub mod report;
pub mod sources;
pub mod wiring;

pub use config::VaultspeedSettings;
