//! `vdp maintenance export` (`get_db_maintenance_json.py`): runs the maintenance macro and saves its JSON.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `[maintenance]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MaintenanceSettings {
    /// The macro that prints the maintenance report as one JSON object.
    pub macro_name: String,
    /// dbt target to run it against.
    pub target: String,
    /// Output file, relative to the project.
    pub out: String,
}

impl Default for MaintenanceSettings {
    fn default() -> Self {
        Self {
            macro_name: "check_database_maintenance".into(),
            target: "run".into(),
            out: "maintenance.json".into(),
        }
    }
}

impl MaintenanceSettings {
    pub const SECTION: &'static str = "maintenance";

    /// The dbt arguments (`--quiet run-operation <macro> --target <target>`).
    pub fn args(&self) -> Vec<String> {
        vec![
            "--quiet".into(),
            "run-operation".into(),
            self.macro_name.clone(),
            "--target".into(),
            self.target.clone(),
        ]
    }
}

/// The last JSON object printed on its own line (from its first `{`), as the Python script read it.
pub fn report_from_output(stdout: &str) -> Option<Value> {
    stdout.lines().rev().find_map(|line| {
        let line = line.trim();
        let start = line.find('{')?;
        if !line.ends_with('}') {
            return None;
        }
        serde_json::from_str::<Value>(&line[start..])
            .ok()
            .filter(Value::is_object)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_printed_object() {
        let out = "   dbt-oss 2.0.11\nnoise\n12:00 {\"tables\": {\"a\": 1}}\n\nFinished\n";
        assert_eq!(report_from_output(out).unwrap()["tables"]["a"], 1);
        assert!(report_from_output("no json here\n").is_none());
    }
}
