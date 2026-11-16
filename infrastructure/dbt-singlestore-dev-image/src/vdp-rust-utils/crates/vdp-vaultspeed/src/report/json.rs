use std::fs;
use std::path::PathBuf;

use vdp_vaultspeed_domain::RunReport;

use crate::report::error::io;
use crate::report::{ReportError, ReportSink};

/// Writes the full report (every decision, statement and DAG diff) as JSON. In preview
/// mode this file *is* the execution plan.
#[derive(Debug, Clone)]
pub struct JsonReportSink {
    target: Target,
}

#[derive(Debug, Clone)]
enum Target {
    Directory(PathBuf),
    File(PathBuf),
}

impl JsonReportSink {
    /// `<dir>/vsaf_<ENV>_<run id>.json`
    pub const fn in_dir(dir: PathBuf) -> Self {
        Self {
            target: Target::Directory(dir),
        }
    }

    pub const fn to_file(path: PathBuf) -> Self {
        Self {
            target: Target::File(path),
        }
    }
}

impl ReportSink for JsonReportSink {
    fn write(&self, report: &RunReport) -> Result<Vec<PathBuf>, ReportError> {
        let path = match &self.target {
            Target::File(path) => path.clone(),
            Target::Directory(dir) => dir.join(format!(
                "vsaf_{}_{}.json",
                report.environment, report.run_id
            )),
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io(dir))?;
        }
        fs::write(&path, serde_json::to_vec_pretty(report)?).map_err(io(&path))?;
        Ok(vec![path])
    }
}
