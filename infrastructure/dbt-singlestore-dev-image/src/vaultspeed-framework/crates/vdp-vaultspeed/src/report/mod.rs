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
    /// Writes the report and returns the files it created.
    ///
    /// # Errors
    ///
    /// [`ReportError::Io`] when a file or directory cannot be written, or the output cannot be flushed; [`ReportError::Csv`]
    /// when the CSV cannot be written; [`ReportError::Json`] when the report cannot be serialised.
    fn write(&self, report: &RunReport) -> Result<Vec<PathBuf>, ReportError>;
}
