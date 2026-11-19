//! Manifest analyses (`scripts/qa`).
//!
//! A small domain module: pure analyses over dbt's `manifest.json` (read with [`vdp_dbt::artifacts`]) plus one query for
//! the timings of the latest run. `vdp qa` in `vdp-cli` prints them.
//!
//! # Responsibilities
//!
//! - [`orphan_tables`]: table models nothing reads (`detect_tables_with_no_deps.py`). Tests no longer count as readers:
//!   the Python script counted every node, so a table with only a `not_null` test looked used.
//! - [`view_candidates`]: views whose downstream tables take long to build, so materializing the view may pay off
//!   (`analyze_views_downstream_timings.py`), with the same traversal, ranking and CSV columns.
//! - [`latest_timings`]: the execution times of the latest run, by unique id, from the run-results table.
//! - [`candidates_csv`]: the Python CSV, with the same columns.
//!
//! # Example
//!
//! ```
//! use vdp_dbt::artifacts::Manifest;
//!
//! // An empty manifest has nothing to report.
//! let manifest = Manifest::default();
//! assert!(vdp_qa::orphan_tables(&manifest).is_empty());
//! assert!(vdp_qa::view_candidates(&manifest, &std::collections::HashMap::new()).is_empty());
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::BuildHasher;

use serde::{Deserialize, Serialize};
use vdp_db::{DbError, SqlSession};
use vdp_dbt::artifacts::{Manifest, Node};

/// `[qa]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct QaSettings {
    /// Table with the run results of past dbt runs (`unique_id`, `execution_time`, `run_started_at`), as
    /// `database.table`, for projects whose management database differs per environment. `{env}` is the lower-case
    /// environment (`ldv`, `abt`, `cbt`, `prd`) and `{env_short}` its short name (`dev`, `val`, `cbt`, `prd`):
    /// `{env_short}_vdp_mgmt.dbt_run_results` is `dev_vdp_mgmt.dbt_run_results` on `LDV`.
    pub run_results_table: String,
}

impl Default for QaSettings {
    fn default() -> Self {
        Self {
            run_results_table: "vdp_mgmt.dbt_run_results".into(),
        }
    }
}

impl QaSettings {
    /// The name of the config section these settings are read from (`[qa]`).
    pub const SECTION: &'static str = "qa";

    /// [`QaSettings::run_results_table`] for an environment: `environment` is its name (rendered lower-case for
    /// `{env}`) and `short_name` its short name (for `{env_short}`, as `vdp_core::Environment::short_name` gives it).
    pub fn run_results_table_for(&self, environment: &str, short_name: &str) -> String {
        self.run_results_table
            .replace("{env}", &environment.to_ascii_lowercase())
            .replace("{env_short}", short_name)
    }
}

fn materialized(node: &Node) -> String {
    node.config
        .materialized
        .clone()
        .unwrap_or_default()
        .to_lowercase()
}

fn is_model(node: &Node) -> bool {
    node.resource_type == "model"
}

/// Table models that no other node, tests excluded, depends on. Sorted by unique id.
pub fn orphan_tables(manifest: &Manifest) -> Vec<String> {
    let used: BTreeSet<&str> = manifest
        .nodes
        .values()
        .filter(|n| !matches!(n.resource_type.as_str(), "test" | "unit_test"))
        .flat_map(|n| n.depends_on.nodes.iter().map(String::as_str))
        .collect();
    let mut orphans: Vec<String> = manifest
        .nodes
        .values()
        .filter(|n| {
            is_model(n) && materialized(n) == "table" && !used.contains(n.unique_id.as_str())
        })
        .map(|n| n.unique_id.clone())
        .collect();
    orphans.sort();
    orphans
}

/// One row of `dbt_view_candidates.csv`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    pub model_name: String,
    pub file_path: String,
    pub affected_downstream_count: usize,
    pub total_downstream_time_seconds: f64,
    /// `{"table_model": seconds, ...}`, in traversal order.
    pub downstream_timings_json: String,
    pub dbt_selector: String,
}

fn children_of(manifest: &Manifest) -> HashMap<&str, Vec<&str>> {
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for (id, node) in &manifest.nodes {
        for parent in &node.depends_on.nodes {
            children
                .entry(parent.as_str())
                .or_default()
                .push(id.as_str());
        }
    }
    children
}

/// Downstream models of `start`: views are followed, the first table (or other model) on each branch ends it.
fn downstream<'a>(
    start: &'a str,
    manifest: &'a Manifest,
    children: &HashMap<&'a str, Vec<&'a str>>,
) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        for child in children.get(current).into_iter().flatten() {
            if seen.contains(child) {
                continue;
            }
            let Some(node) = manifest.nodes.get(*child) else {
                continue;
            };
            if is_model(node) {
                seen.insert(*child);
                found.push(*child);
                if materialized(node) == "view" {
                    queue.push_back(child);
                }
            }
        }
    }
    found
}

/// Rounds to two decimals. Never returns `-0.0`: an empty `f64` sum is `-0.0` on current Rust, and a small negative
/// value rounds to it, which would print as `-0.00`.
fn round2(value: f64) -> f64 {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// Views ranked by the build time of the tables downstream of them. Views with a `t_<view>` table counterpart
/// and views without downstream models are left out.
pub fn view_candidates<S: BuildHasher>(
    manifest: &Manifest,
    timings: &HashMap<String, f64, S>,
) -> Vec<Candidate> {
    let tables: BTreeSet<String> = manifest
        .nodes
        .values()
        .filter(|n| is_model(n) && matches!(materialized(n).as_str(), "table" | "incremental"))
        .map(|n| n.name.to_lowercase())
        .collect();
    let children = children_of(manifest);
    let mut candidates = Vec::new();
    for (id, node) in &manifest.nodes {
        if !is_model(node) || materialized(node) != "view" {
            continue;
        }
        if tables.contains(&format!("t_{}", node.name).to_lowercase()) {
            continue;
        }
        let affected = downstream(id, manifest, &children);
        if affected.is_empty() {
            continue;
        }
        let mut table_timings: Vec<(String, f64)> = Vec::new();
        let mut names = Vec::new();
        for child_id in &affected {
            let child = &manifest.nodes[*child_id];
            names.push(child.name.clone());
            if matches!(materialized(child).as_str(), "table" | "incremental") {
                let seconds = round2(timings.get(*child_id).copied().unwrap_or(0.0));
                match table_timings.iter_mut().find(|(n, _)| *n == child.name) {
                    Some(entry) => entry.1 = seconds,
                    None => table_timings.push((child.name.clone(), seconds)),
                }
            }
        }
        // `round2` turns the `-0.0` of an empty sum into `0.0`.
        let total = round2(table_timings.iter().map(|(_, s)| s).sum());
        // In traversal order, like the Python dict (serde_json's map would sort the keys).
        let json = format!(
            "{{{}}}",
            table_timings
                .iter()
                .map(|(name, seconds)| format!(
                    "{}: {}",
                    serde_json::Value::String(name.clone()),
                    python_json(&serde_json::json!(seconds))
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        candidates.push(Candidate {
            model_name: node.name.clone(),
            file_path: node.original_file_path.clone(),
            affected_downstream_count: affected.len(),
            total_downstream_time_seconds: total,
            downstream_timings_json: json,
            dbt_selector: format!("{} {}", node.name, names.join(" ")),
        });
    }
    // Highest first; stable for equal times (in manifest order, as Python's sorted()).
    candidates.sort_by(|a, b| {
        b.total_downstream_time_seconds
            .total_cmp(&a.total_downstream_time_seconds)
    });
    candidates
}

/// `json.dumps(value, indent=None)`: `", "` and `": "` separators, floats with a decimal point.
fn python_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!(
                    "{}: {}",
                    serde_json::Value::String(k.clone()),
                    python_json(v)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && !n.is_i64() => format!("{f:.1}"),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// Execution times of the latest run, by unique id.
///
/// # Errors
///
/// [`DbError`] when the query fails. Rows without a text id are skipped.
#[allow(
    clippy::cast_precision_loss,
    reason = "execution times in seconds are far below 2^52"
)]
pub fn latest_timings(
    session: &dyn SqlSession,
    table: &str,
) -> Result<HashMap<String, f64>, DbError> {
    let quoted = table
        .split('.')
        .map(|p| format!("`{}`", p.replace('`', "")))
        .collect::<Vec<_>>()
        .join(".");
    let sql = format!(
        "SELECT unique_id, execution_time FROM {quoted} \
         WHERE run_started_at = (SELECT MAX(run_started_at) FROM {quoted})"
    );
    let rows = session.query(&sql, &[])?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let mut values = row.into_iter();
            let id = values.next()?.as_text()?.to_owned();
            let seconds = match values.next()? {
                vdp_db::SqlValue::Float(f) => f,
                vdp_db::SqlValue::Int(i) => i as f64,
                vdp_db::SqlValue::UInt(u) => u as f64,
                vdp_db::SqlValue::Text(t) => t.parse().unwrap_or(0.0),
                vdp_db::SqlValue::Null => 0.0,
            };
            Some((id, seconds))
        })
        .collect())
}

/// Writes the candidates as the Python CSV (same columns).
pub fn candidates_csv(candidates: &[Candidate]) -> String {
    let quote = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_owned()
        }
    };
    let mut out = String::from(
        "model_name,file_path,affected_downstream_count,total_downstream_time_seconds,downstream_timings_json,dbt_selector\r\n",
    );
    for c in candidates {
        let fields = [
            quote(&c.model_name),
            quote(&c.file_path),
            c.affected_downstream_count.to_string(),
            python_json(&serde_json::json!(c.total_downstream_time_seconds)),
            quote(&c.downstream_timings_json),
            quote(&c.dbt_selector),
        ];
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Groups for display: candidates by file directory.
pub fn by_directory(candidates: &[Candidate]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for c in candidates {
        let dir = c
            .file_path
            .rsplit_once('/')
            .map_or("", |(d, _)| d)
            .to_owned();
        *counts.entry(dir).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the SQL it is asked and answers one run.
    #[derive(Debug)]
    struct RunResults(std::sync::Mutex<Vec<String>>);

    impl SqlSession for RunResults {
        fn execute(&self, _: &str) -> Result<(), DbError> {
            Ok(())
        }
        fn execute_with(&self, _: &str, _: &[vdp_db::SqlValue]) -> Result<(), DbError> {
            Ok(())
        }
        fn query(
            &self,
            sql: &str,
            _: &[vdp_db::SqlValue],
        ) -> Result<Vec<Vec<vdp_db::SqlValue>>, DbError> {
            self.0.lock().unwrap().push(sql.to_owned());
            Ok(vec![vec![
                vdp_db::SqlValue::Text("model.p.t1".into()),
                vdp_db::SqlValue::Float(12.5),
            ]])
        }
    }

    /// The dbt workshop keeps its run results in `<env>_vdp_mgmt`; the built-in default stays `vdp_mgmt`.
    #[test]
    fn the_run_results_table_follows_the_configuration_and_environment() {
        let default = QaSettings::default();
        assert_eq!(
            default.run_results_table_for("LDV", "dev"),
            "vdp_mgmt.dbt_run_results"
        );
        let workshop = QaSettings {
            run_results_table: "{env}_vdp_mgmt.dbt_run_results".into(),
        };
        let table = workshop.run_results_table_for("dev", "dev");
        assert_eq!(table, "dev_vdp_mgmt.dbt_run_results");
        // `{env}` is the canonical name, `{env_short}` the short one.
        let both = QaSettings {
            run_results_table: "{env}_{env_short}.t".into(),
        };
        assert_eq!(both.run_results_table_for("LDV", "dev"), "ldv_dev.t");
        assert_eq!(both.run_results_table_for("CBT", "cbt"), "cbt_cbt.t");
        let short = QaSettings {
            run_results_table: "{env_short}_vdp_mgmt.dbt_run_results".into(),
        };
        assert_eq!(
            short.run_results_table_for("ABT", "val"),
            "val_vdp_mgmt.dbt_run_results"
        );
        let session = RunResults(std::sync::Mutex::new(Vec::new()));
        let timings = latest_timings(&session, &table).unwrap();
        assert!((timings["model.p.t1"] - 12.5).abs() < f64::EPSILON);
        let sql = session.0.lock().unwrap().join("\n");
        assert!(
            sql.contains("FROM `dev_vdp_mgmt`.`dbt_run_results`"),
            "{sql}"
        );
        assert!(!sql.contains("FROM `vdp_mgmt`"), "{sql}");
    }

    fn manifest() -> Manifest {
        serde_json::from_str(
            r#"{"nodes":{
              "model.p.v":{"unique_id":"model.p.v","resource_type":"model","name":"v","config":{"materialized":"view"},"original_file_path":"models/v.sql"},
              "model.p.v2":{"unique_id":"model.p.v2","resource_type":"model","name":"v2","config":{"materialized":"view"},"depends_on":{"nodes":["model.p.v"]}},
              "model.p.t1":{"unique_id":"model.p.t1","resource_type":"model","name":"t1","config":{"materialized":"table"},"depends_on":{"nodes":["model.p.v2"]}},
              "model.p.t2":{"unique_id":"model.p.t2","resource_type":"model","name":"t2","config":{"materialized":"incremental"},"depends_on":{"nodes":["model.p.v"]}},
              "model.p.after":{"unique_id":"model.p.after","resource_type":"model","name":"after","config":{"materialized":"table"},"depends_on":{"nodes":["model.p.t1"]}},
              "test.p.nn":{"unique_id":"test.p.nn","resource_type":"test","name":"nn","depends_on":{"nodes":["model.p.after"]}}
            }}"#,
        )
        .unwrap()
    }

    #[test]
    fn orphans_ignore_tests() {
        assert_eq!(orphan_tables(&manifest()), ["model.p.after"]);
    }

    /// A view whose only downstream model is not a table has no timings; its total must be `0.0`, not `-0.0`.
    #[test]
    fn a_total_without_timings_is_positive_zero() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"nodes":{
              "model.p.v":{"unique_id":"model.p.v","resource_type":"model","name":"v","config":{"materialized":"view"},"original_file_path":"models/v.sql"},
              "model.p.e":{"unique_id":"model.p.e","resource_type":"model","name":"e","config":{"materialized":"ephemeral"},"depends_on":{"nodes":["model.p.v"]}}
            }}"#,
        )
        .unwrap();
        let candidates = view_candidates(&manifest, &HashMap::new());
        assert_eq!(candidates.len(), 1);
        let total = candidates[0].total_downstream_time_seconds;
        assert!(total.is_sign_positive(), "total is {total:?}");
        assert!(!format!("{total:.2}").contains('-'));
        assert!(!candidates_csv(&candidates).contains("-0.0"));
        assert!(round2(-0.001).is_sign_positive());
    }

    #[test]
    fn candidates_stop_at_tables() {
        let timings = HashMap::from([
            ("model.p.t1".to_owned(), 2.004),
            ("model.p.t2".to_owned(), 3.0),
        ]);
        let candidates = view_candidates(&manifest(), &timings);
        assert_eq!(candidates[0].model_name, "v");
        assert_eq!(candidates[0].affected_downstream_count, 3);
        assert!((candidates[0].total_downstream_time_seconds - 5.0).abs() < 1e-9);
        assert_eq!(
            candidates[0].downstream_timings_json,
            r#"{"t2": 3.0, "t1": 2.0}"#
        );
        // Breadth first; siblings in unique-id order.
        assert_eq!(candidates[0].dbt_selector, "v t2 v2 t1");
        assert_eq!(candidates[1].model_name, "v2");
    }
}
