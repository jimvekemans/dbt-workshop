use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use vdp_vaultspeed_domain::{Decision, RunReport, StatementOutcome};

use crate::report::error::io;
use crate::report::{ReportError, ReportSink};

/// `JUnit` XML so GitLab shows every executed group as a test on the pipeline page.
#[derive(Debug, Clone)]
pub struct JunitSink {
    path: PathBuf,
}

impl JunitSink {
    /// Writes the XML to `path`.
    pub const fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ReportSink for JunitSink {
    fn write(&self, report: &RunReport) -> Result<Vec<PathBuf>, ReportError> {
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites name=\"vsaf\">\n",
        );
        for source in &report.sources {
            let failures =
                source.groups.iter().filter(|g| g.failed()).count() + source.errors.len();
            let skipped = source
                .groups
                .iter()
                .filter(|g| !g.decision.is_execute())
                .count();
            let _ = writeln!(
                xml,
                "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{failures}\" skipped=\"{skipped}\">",
                escape(&source.source),
                source.groups.len() + source.errors.len() + source.dags.len(),
            );
            for group in &source.groups {
                let _ = write!(
                    xml,
                    "    <testcase classname=\"{}.{}\" name=\"{}\">",
                    escape(&source.source),
                    escape(&group.phase),
                    escape(&group.group_id)
                );
                match &group.decision {
                    Decision::Skip(_) => {
                        let _ = write!(
                            xml,
                            "<skipped message=\"{}\"/>",
                            escape(&group.decision.to_string())
                        );
                    }
                    Decision::Refuse { reason } => {
                        let _ = write!(
                            xml,
                            "<failure message=\"refused\">{}</failure>",
                            escape(reason)
                        );
                    }
                    Decision::Execute(_) => {
                        for statement in &group.statements {
                            if let StatementOutcome::Failed { code, message } = &statement.outcome {
                                let _ = write!(
                                    xml,
                                    "<failure message=\"{}\">{}</failure>",
                                    escape(&format!(
                                        "{} {message}",
                                        code.map(|c| c.to_string()).unwrap_or_default()
                                    )),
                                    escape(&statement.effective_sql)
                                );
                            }
                        }
                    }
                }
                xml.push_str("</testcase>\n");
            }
            for dag in &source.dags {
                let _ = write!(
                    xml,
                    "    <testcase classname=\"{}.dags\" name=\"{}\">",
                    escape(&source.source),
                    escape(&dag.target_file)
                );
                if let Some(error) = &dag.error {
                    let _ = write!(xml, "<failure message=\"{}\"/>", escape(error));
                }
                xml.push_str("</testcase>\n");
            }
            for error in &source.errors {
                let _ = writeln!(
                    xml,
                    "    <testcase classname=\"{}\" name=\"source\"><failure message=\"{}\"/></testcase>",
                    escape(&source.source),
                    escape(error)
                );
            }
            xml.push_str("  </testsuite>\n");
        }
        xml.push_str("</testsuites>\n");
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(io(dir))?;
        }
        fs::write(&self.path, xml).map_err(io(&self.path))?;
        Ok(vec![self.path.clone()])
    }
}

fn escape(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .fold(String::with_capacity(text.len()), |mut out, c| {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                c => out.push(c),
            }
            out
        })
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn escapes_xml() {
        assert_eq!(escape("a<b & \"c\"\u{1}"), "a&lt;b &amp; &quot;c&quot;");
    }
}
