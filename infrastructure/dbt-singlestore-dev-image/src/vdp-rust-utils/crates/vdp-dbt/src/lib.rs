//! Driving the dbt v2 binary: the `dbt` wrapper, the flag grammar and the artifacts dbt writes.
//!
//! A building block used by the CLI entry point and by modules that run dbt (source maintenance, the QA checks).
//! Modules depend on the [`DbtEngine`] port, never on the subprocess.
//!
//! # Responsibilities
//!
//! - [`flags`]: dbt's command-line grammar, derived from the binary itself.
//! - [`argv`]: reads an argv as dbt's clap parser does, and fixes only `--args`/`--vars` values dbt would misread or
//!   reject.
//! - [`guard`]: the "no unselected `build`/`run`" rule of the Python wrapper, without its bypasses.
//! - [`engine`]: the [`DbtEngine`] port and the subprocess adapter (signals forwarded, secrets only in the child).
//! - [`missing`]: undefined sources (dbt1048) in dbt's output.
//! - [`wrapper`]: the `dbt` command itself.
//!
//! # Example
//!
//! ```
//! use vdp_dbt::DbtSettings;
//!
//! let settings = DbtSettings::default();
//! // Whole-project `dbt build` is allowed for service users only.
//! assert!(settings.is_service_user("airflow_dbt"));
//! assert!(!settings.is_service_user("pu_jim_dbt"));
//! ```

pub mod argv;
pub mod artifacts;
pub mod engine;
mod error;
pub mod flags;
pub mod guard;
mod locate;
pub mod missing;
pub mod profile;
mod project;
mod settings;
pub mod wrapper;

pub use engine::{Completed, DbtEngine, Invocation, SubprocessDbt};
pub use error::DbtError;
pub use flags::Grammar;
pub use locate::locate_engine;
pub use missing::{MissingSource, missing_sources};
pub use project::DbtProject;
pub use settings::DbtSettings;
