//! `[vaultspeed]` settings and the framework's `config/vaultspeed.json` (FMC sources).
//!
//! Every value has a default reproducing the Python framework, so an empty `[vaultspeed]` section behaves like
//! `python/deploy.py`; deviations are opt-in or documented on the field. This module is plain data: turning
//! settings into components happens in [`crate::wiring`].

mod error;
mod fmc_sources;
mod model;

pub use error::ConfigError;
pub use fmc_sources::load_fmc_sources;
pub use model::{
    DagSettings, DefaultArg, EnvironmentSettings, FailurePolicySetting, FailureSettings,
    HistorySettings, LedgerBackend, LedgerSettings, PathSettings, PlanningSettings,
    SnapshotSetting, SnapshotSettings, StandaloneDropsSetting, VaultspeedSettings,
};
