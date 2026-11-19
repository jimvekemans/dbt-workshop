use std::fs;
use std::path::PathBuf;

use vdp_vaultspeed_domain::{RunReport, StatementOutcome};

use crate::report::error::io;
use crate::report::{ReportError, ReportSink};

/// Writes `<logs>/<source>/deploy_errors_<yy-mm-dd@HH:MM:SS>.csv` for every source with
/// failed statements, in the Python framework's format (`JOB_NAME, TYPE, JOB_QUERY,
/// ERROR_MESSAGE`, all quoted) but with the real database error message.
#[derive(Debug, Clone)]
pub struct ErrorCsvSink {
    logs_dir: PathBuf,
}

impl ErrorCsvSink {
    /// Creates the sink; the files go under `logs_dir`.
    pub const fn new(logs_dir: PathBuf) -> Self {
        Self { logs_dir }
    }
}

impl ReportSink for ErrorCsvSink {
    fn write(&self, report: &RunReport) -> Result<Vec<PathBuf>, ReportError> {
        let stamp = report.started_at.format("%y-%m-%d@%H:%M:%S");
        let mut written = Vec::new();
        for source in &report.sources {
            let failures: Vec<_> = source
                .groups
                .iter()
                .flat_map(|g| g.statements.iter().map(move |s| (g, s)))
                .filter_map(|(g, s)| match &s.outcome {
                    StatementOutcome::Failed { code, message } => {
                        let message =
                            code.map_or_else(|| message.clone(), |c| format!("({c}) {message}"));
                        Some((g, s, message))
                    }
                    _ => None,
                })
                .collect();
            if failures.is_empty() {
                continue;
            }
            let dir = self.logs_dir.join(&source.source);
            fs::create_dir_all(&dir).map_err(io(&dir))?;
            let path = dir.join(format!("deploy_errors_{stamp}.csv"));
            let csv_error = |source| ReportError::Csv {
                path: path.clone(),
                source,
            };
            let mut writer = csv::WriterBuilder::new()
                .quote_style(csv::QuoteStyle::Always)
                .from_path(&path)
                .map_err(csv_error)?;
            writer
                .write_record(["JOB_NAME", "TYPE", "JOB_QUERY", "ERROR_MESSAGE"])
                .map_err(csv_error)?;
            for (group, statement, message) in failures {
                writer
                    .write_record([
                        group.job.as_str(),
                        group.artifact.as_str(),
                        &statement.effective_sql,
                        &message,
                    ])
                    .map_err(csv_error)?;
            }
            writer.flush().map_err(io(&path))?;
            written.push(path);
        }
        Ok(written)
    }
}
