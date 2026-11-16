//! `vdp seeds refresh-metadata` (`refresh_dbt_metadata_seeds.py`): seeds listing the project's models and
//! sources, so the database knows what dbt owns.
//!
//! The Python script ran `dbt list --output json` twice; the same `database`/`name` pairs come straight from
//! the manifest of one `dbt parse`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::artifacts::Manifest;

/// `[seeds]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SeedSettings {
    /// Directory of the metadata seeds, relative to the project.
    pub dir: String,
    /// Database (and schema) the seeds load into.
    pub database: String,
}

impl Default for SeedSettings {
    fn default() -> Self {
        Self {
            dir: "seeds/meta".into(),
            database: "vdp_mgmt".into(),
        }
    }
}

impl SeedSettings {
    pub const SECTION: &'static str = "seeds";
}

fn csv(rows: impl Iterator<Item = (String, String)>) -> String {
    let mut lines: Vec<String> = rows.map(|(db, name)| format!("{db},{name}\n")).collect();
    lines.sort();
    format!("table_schema,table_name\n{}", lines.concat())
}

/// The seed properties. The Python template also had `database:`/`schema:` next to `name:`; dbt v2 rejects
/// those keys (dbt1060, "Ignored unexpected key"), so they are only set under `config:`.
fn seed_yml(name: &str, what: &str, database: &str) -> String {
    format!(
        "\nversion: 2\nseeds:\n  - name: {name}\n    description: |\n      This seed contains the list of dbt {what} in the project.\n    config:\n      database: {database}\n      schema: {database}\n      delimiter: \",\"\n      column_types:\n        table_schema: varchar(256)\n        table_name: varchar(256)\n"
    )
}

/// The two property files. They do not depend on the project, so they are written before `dbt parse`: a
/// repository with the old Python-generated files (rejected by dbt v2) then parses again.
pub fn render_properties(project_root: &Path, settings: &SeedSettings) -> Vec<(PathBuf, String)> {
    let dir = project_root.join(&settings.dir);
    vec![
        (
            dir.join("dbt_models.yml"),
            seed_yml("dbt_models", "models", &settings.database),
        ),
        (
            dir.join("dbt_sources.yml"),
            seed_yml("dbt_sources", "sources", &settings.database),
        ),
    ]
}

/// The two CSV files from the manifest.
pub fn render_csv(
    manifest: &Manifest,
    project_root: &Path,
    settings: &SeedSettings,
) -> Vec<(PathBuf, String)> {
    let dir = project_root.join(&settings.dir);
    let models = manifest
        .nodes_of(&["model", "seed", "snapshot"])
        .map(|n| (n.database.clone().unwrap_or_default(), n.name.clone()));
    let sources = manifest
        .sources
        .values()
        .map(|s| (s.database.clone().unwrap_or_default(), s.name.clone()));
    vec![
        (dir.join("dbt_models.csv"), csv(models)),
        (dir.join("dbt_sources.csv"), csv(sources)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_files_as_the_python_script() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"nodes":{
                "model.p.b":{"unique_id":"model.p.b","resource_type":"model","name":"b","database":"db1"},
                "seed.p.a":{"unique_id":"seed.p.a","resource_type":"seed","name":"a","database":"db2"},
                "test.p.t":{"unique_id":"test.p.t","resource_type":"test","name":"t","database":"db1"}},
               "sources":{"source.p.s.x":{"unique_id":"source.p.s.x","source_name":"s","name":"x","database":"raw"}}}"#,
        )
        .unwrap();
        let files = render_csv(&manifest, Path::new("/p"), &SeedSettings::default());
        assert_eq!(files[0].0, PathBuf::from("/p/seeds/meta/dbt_models.csv"));
        assert_eq!(files[0].1, "table_schema,table_name\ndb1,b\ndb2,a\n");
        assert_eq!(files[1].1, "table_schema,table_name\nraw,x\n");
        let files = render_properties(Path::new("/p"), &SeedSettings::default());
        assert!(
            files[0]
                .1
                .contains("  - name: dbt_models\n    description: |\n")
        );
        assert!(
            files[0]
                .1
                .contains("    config:\n      database: vdp_mgmt\n      schema: vdp_mgmt\n")
        );
    }
}
