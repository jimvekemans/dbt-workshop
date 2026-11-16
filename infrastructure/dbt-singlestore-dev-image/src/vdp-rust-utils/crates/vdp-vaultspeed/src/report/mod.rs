//! Rendering of a [`vdp_vaultspeed_domain::RunReport`] for people and machines.

mod console;
mod csv_errors;
mod error;
mod json;
mod junit;

use std::fmt::Debug;
use std::path::PathBuf;

pub use console::ConsoleSink;
pub use csv_errors::ErrorCsvSink;
pub use error::ReportError;
pub use json::JsonReportSink;
pub use junit::JunitSink;
use vdp_vaultspeed_domain::RunReport;

/// Writes a finished run report somewhere; returns the files it created.
pub trait ReportSink: Send + Sync + Debug {
    fn write(&self, report: &RunReport) -> Result<Vec<PathBuf>, ReportError>;
}
