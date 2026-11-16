//! Which YAML file declares which source.

use std::path::{Path, PathBuf};

use crate::tree::{self, YNode};
use crate::{SourcesError, SourcesSettings};

/// A `sources:` entry in some file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDecl {
    pub file: PathBuf,
    pub name: String,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub tables: Vec<String>,
}

impl SourceDecl {
    /// The SingleStore database holding the tables: `database`, else `schema`, else the source name. `None` when
    /// it is computed by Jinja and only dbt can tell.
    pub fn physical_database(&self) -> Option<String> {
        self.physical_database_with(&SourcesSettings::default())
    }

    /// Same, reading Jinja through `[sources.database_prefix]` ([`SourcesSettings::physical_of`]).
    pub fn physical_database_with(&self, settings: &SourcesSettings) -> Option<String> {
        let value = self
            .database
            .clone()
            .or_else(|| self.schema.clone())
            .unwrap_or_else(|| self.name.clone());
        settings.physical_of(&value)
    }

    pub fn declares(&self, table: &str) -> bool {
        self.tables.iter().any(|t| t == table)
    }
}

/// Every source declared under the model paths.
#[derive(Debug, Clone, Default)]
pub struct SourceIndex {
    pub sources: Vec<SourceDecl>,
}

impl SourceIndex {
    pub fn scan(model_paths: &[PathBuf]) -> Result<Self, SourcesError> {
        let mut files = Vec::new();
        for dir in model_paths {
            collect_yaml(dir, &mut files);
        }
        files.sort();
        let mut sources = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file).map_err(|source| SourcesError::Io {
                path: file.clone(),
                source,
            })?;
            if !text.contains("sources") {
                continue;
            }
            let root = tree::parse(&text).map_err(|detail| SourcesError::Yaml {
                path: file.clone(),
                detail,
            })?;
            if let Some(root) = root {
                sources.extend(declarations(&file, &root));
            }
        }
        Ok(Self { sources })
    }

    pub fn find(&self, name: &str) -> Option<&SourceDecl> {
        self.sources.iter().find(|s| s.name == name)
    }
}

pub(crate) fn declarations(file: &Path, root: &YNode) -> Vec<SourceDecl> {
    let Some(list) = root.get("sources") else {
        return Vec::new();
    };
    list.items()
        .iter()
        .filter_map(|source| {
            let text = |key: &str| source.get(key).and_then(YNode::as_str).map(str::to_owned);
            Some(SourceDecl {
                file: file.to_owned(),
                name: text("name")?,
                database: text("database"),
                schema: text("schema"),
                tables: source
                    .get("tables")
                    .map(|t| {
                        t.items()
                            .iter()
                            .filter_map(|table| {
                                table.get("name").and_then(YNode::as_str).map(str::to_owned)
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yaml(&path, out);
        } else if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_sources_across_files() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(models.join("staging")).unwrap();
        std::fs::write(
            models.join("staging/sources.yml"),
            "sources:\n  - name: raw\n    schema: raw_db\n    tables:\n      - name: a\n      - name: b\n",
        )
        .unwrap();
        std::fs::write(models.join("x.yml"), "models:\n  - name: m\n").unwrap();
        let index = SourceIndex::scan(&[models]).unwrap();
        let raw = index.find("raw").unwrap();
        assert_eq!(raw.tables, ["a", "b"]);
        assert_eq!(raw.physical_database().as_deref(), Some("raw_db"));
        assert!(index.find("other").is_none());
    }
}
