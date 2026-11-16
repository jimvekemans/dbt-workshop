//! The parts of `target/manifest.json` (v12, as dbt v2 writes it) the modules read.
//!
//! Hand-written serde subsets instead of dbt's own `dbt-schemas` crate: that crate is unpublished and pulls in
//! the whole dbt workspace. Unknown fields are ignored.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::DbtError;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub nodes: BTreeMap<String, Node>,
    #[serde(default)]
    pub sources: BTreeMap<String, Source>,
    #[serde(default)]
    pub macros: BTreeMap<String, Macro>,
    #[serde(default)]
    pub child_map: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub parent_map: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Node {
    pub unique_id: String,
    pub resource_type: String,
    pub name: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub package_name: String,
    #[serde(default)]
    pub original_file_path: String,
    #[serde(default)]
    pub config: NodeConfig,
    #[serde(default)]
    pub depends_on: DependsOn,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeConfig {
    #[serde(default)]
    pub materialized: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DependsOn {
    #[serde(default)]
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Source {
    pub unique_id: String,
    pub source_name: String,
    pub name: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub package_name: String,
    #[serde(default)]
    pub original_file_path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Macro {
    #[serde(default)]
    pub macro_sql: String,
    #[serde(default)]
    pub package_name: String,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self, DbtError> {
        let text = std::fs::read_to_string(path).map_err(|e| DbtError::Artifact {
            path: path.to_owned(),
            detail: e.to_string(),
        })?;
        serde_json::from_str(&text).map_err(|e| DbtError::Artifact {
            path: path.to_owned(),
            detail: e.to_string(),
        })
    }

    /// Nodes of the given resource types (`model`, `seed`, `snapshot`, ...).
    pub fn nodes_of<'a>(&'a self, types: &'a [&str]) -> impl Iterator<Item = &'a Node> + 'a {
        self.nodes
            .values()
            .filter(move |n| types.contains(&n.resource_type.as_str()))
    }
}
