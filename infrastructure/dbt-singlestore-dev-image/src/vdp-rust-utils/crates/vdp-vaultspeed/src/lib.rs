//! VaultSpeed deployments: what the VaultSpeed Automation Framework did as the separate `vsaf` workspace, now a
//! vdp module (`vdp vaultspeed …`).
//!
//! It deploys staged **PML scripts** and **VaultSpeed generations** (DDL, ETL, COMP, FMC) to SingleStore and
//! Airflow and knows, per environment, exactly what already ran (the ledger). The modules keep the boundaries the
//! separate crates had: planners read the ledger only through [`ledger::LedgerReader`], the engine depends on
//! traits only, and [`wiring`] is the one place that picks implementations (the CLI and the release hub both use
//! it, so they plan the same way).

pub mod config;
pub mod engine;
pub mod executor;
pub mod ledger;
pub mod planner;
pub mod report;
pub mod sources;
pub mod wiring;

pub use config::VaultspeedSettings;
