//! Generated `sources_<database>.yml` files: one source per database, named after it, tables sorted.
//!
//! The toolkit owns these files and rewrites them, but keeps what people added to tables and columns
//! (descriptions, tests, meta, config): the Python utilities dropped all of that on every rewrite.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::doc::Doc;
use crate::editor::TableEntry;
use crate::{SourceTarget, SourcesError, tree};

pub fn path(generated_dir: &Path, database: &str) -> PathBuf {
    generated_dir.join(format!("sources_{database}.yml"))
}

/// Whether `file` is one of the generated files.
pub fn is_generated(generated_dir: &Path, file: &Path) -> bool {
    file.parent() == Some(generated_dir)
        && file.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
            n.starts_with("sources_")
                && Path::new(n)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("yml"))
        })
}

pub fn load(file: &Path) -> Result<Option<Doc>, SourcesError> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(SourcesError::Io {
                path: file.to_owned(),
                source,
            });
        }
    };
    let yaml_error = |detail: String| SourcesError::Yaml {
        path: file.to_owned(),
        detail,
    };
    match tree::parse(&text).map_err(yaml_error)? {
        None => Ok(None),
        Some(node) => Doc::from_node(&node).map(Some).map_err(yaml_error),
    }
}

pub fn write(file: &Path, doc: &Doc) -> Result<(), SourcesError> {
    let io = |source| SourcesError::Io {
        path: file.to_owned(),
        source,
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(file, doc.emit()).map_err(io)
}

fn empty(target: &SourceTarget) -> Doc {
    Doc::map(vec![
        (
            "version",
            Doc::Scalar {
                text: "2".into(),
                plain: true,
            },
        ),
        ("sources", Doc::Seq(Vec::new())),
    ])
    .with_source(target)
}

trait WithSource {
    fn with_source(self, target: &SourceTarget) -> Self;
}

impl WithSource for Doc {
    fn with_source(mut self, target: &SourceTarget) -> Self {
        source_mut(&mut self, target);
        self
    }
}

/// The source named `target.name` (created with `database = schema = target.database`).
fn source_mut<'a>(doc: &'a mut Doc, target: &SourceTarget) -> &'a mut Doc {
    if doc.get("sources").is_none() {
        doc.set("sources", Doc::Seq(Vec::new()));
    }
    let sources = doc
        .get_mut("sources")
        .and_then(Doc::items_mut)
        .expect("just ensured");
    let index = match sources
        .iter()
        .position(|s| s.name() == Some(target.name.as_str()))
    {
        Some(index) => index,
        None => {
            sources.push(Doc::map(vec![
                ("name", Doc::str(target.name.as_str())),
                ("database", Doc::str(target.database.as_str())),
                ("schema", Doc::str(target.database.as_str())),
                ("tables", Doc::Seq(Vec::new())),
            ]));
            sources.len() - 1
        }
    };
    &mut sources[index]
}

/// The new column list: existing column entries (with their metadata) for columns that still exist, plain
/// `{name}` entries for new ones, in catalog order.
fn merge_columns(existing: Option<&Doc>, columns: &[String]) -> Doc {
    let mut old: BTreeMap<String, Doc> = BTreeMap::new();
    if let Some(Doc::Seq(items)) = existing {
        for item in items {
            if let Some(name) = item.name() {
                old.insert(name.to_owned(), item.clone());
            }
        }
    }
    Doc::Seq(
        columns
            .iter()
            .map(|c| {
                old.remove(c)
                    .unwrap_or_else(|| Doc::map(vec![("name", Doc::str(c.clone()))]))
            })
            .collect(),
    )
}

fn same(database: &str) -> SourceTarget {
    SourceTarget {
        name: database.to_owned(),
        database: database.to_owned(),
    }
}

/// Adds or refreshes tables in the source of `database` (named after it); returns which tables were new.
pub fn upsert(
    doc: Option<Doc>,
    database: &str,
    tables: &[TableEntry],
    include_columns: bool,
) -> (Doc, Vec<String>) {
    upsert_target(doc, &same(database), tables, include_columns)
}

/// [`upsert`] for a source whose name and database differ (`[sources.database_prefix]`).
pub fn upsert_target(
    doc: Option<Doc>,
    target: &SourceTarget,
    tables: &[TableEntry],
    include_columns: bool,
) -> (Doc, Vec<String>) {
    let mut doc = doc.unwrap_or_else(|| empty(target));
    let source = source_mut(&mut doc, target);
    if source.get("tables").is_none() {
        source.set("tables", Doc::Seq(Vec::new()));
    }
    let list = source
        .get_mut("tables")
        .and_then(Doc::items_mut)
        .expect("ensured");
    let mut added = Vec::new();
    for table in tables {
        match list
            .iter_mut()
            .find(|t| t.name() == Some(table.name.as_str()))
        {
            Some(existing) => {
                if include_columns {
                    let merged = merge_columns(existing.get("columns"), &table.columns);
                    existing.set("columns", merged);
                }
            }
            None => {
                list.push(table.to_doc(include_columns));
                added.push(table.name.clone());
            }
        }
    }
    list.sort_by(|a, b| a.name().cmp(&b.name()));
    (doc, added)
}

/// Replaces the tables of the source of `database` with exactly `tables` (keeping metadata of survivors).
pub fn replace_all(
    doc: Option<Doc>,
    database: &str,
    tables: &[TableEntry],
    include_columns: bool,
) -> Doc {
    replace_all_target(doc, &same(database), tables, include_columns)
}

/// [`replace_all`] for a source whose name and database differ (`[sources.database_prefix]`).
pub fn replace_all_target(
    doc: Option<Doc>,
    target: &SourceTarget,
    tables: &[TableEntry],
    include_columns: bool,
) -> Doc {
    let keep: BTreeSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    let mut doc = doc.unwrap_or_else(|| empty(target));
    if let Some(list) = source_mut(&mut doc, target)
        .get_mut("tables")
        .and_then(Doc::items_mut)
    {
        list.retain(|t| t.name().is_some_and(|n| keep.contains(n)));
    }
    upsert_target(Some(doc), target, tables, include_columns).0
}

/// Keeps only the referenced tables of each source; returns `None` when nothing is left.
pub fn trim(mut doc: Doc, referenced: &BTreeSet<(String, String)>) -> Option<Doc> {
    let sources = doc.get_mut("sources").and_then(Doc::items_mut)?;
    for source in sources.iter_mut() {
        let name = source.name().unwrap_or_default().to_owned();
        if let Some(list) = source.get_mut("tables").and_then(Doc::items_mut) {
            list.retain(|t| {
                t.name()
                    .is_some_and(|table| referenced.contains(&(name.clone(), table.to_owned())))
            });
        }
    }
    sources.retain(|s| matches!(s.get("tables"), Some(Doc::Seq(items)) if !items.is_empty()));
    (!sources.is_empty()).then_some(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, columns: &[&str]) -> TableEntry {
        TableEntry {
            name: name.into(),
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    #[test]
    fn upsert_keeps_metadata_and_sorts() {
        let text = "version: 2\nsources:\n  - name: db\n    database: db\n    schema: db\n    tables:\n      - name: b\n        description: keep me\n        columns:\n          - name: id\n            tests: [not_null]\n          - name: gone\n";
        let doc = Doc::from_node(&tree::parse(text).unwrap().unwrap()).unwrap();
        let (doc, added) = upsert(
            Some(doc),
            "db",
            &[table("b", &["id", "new"]), table("a", &["x"])],
            true,
        );
        assert_eq!(added, ["a"]);
        assert_eq!(
            doc.emit(),
            "version: 2\nsources:\n  - name: db\n    database: db\n    schema: db\n    tables:\n      - name: a\n        columns:\n          - name: x\n      - name: b\n        description: keep me\n        columns:\n          - name: id\n            tests: [not_null]\n          - name: new\n"
                .replace("tests: [not_null]", "tests:\n              - not_null")
        );
    }

    #[test]
    fn trim_drops_unreferenced_tables_and_empty_files() {
        let (doc, _) = upsert(None, "db", &[table("a", &[]), table("b", &[])], false);
        let keep: BTreeSet<(String, String)> = [("db".to_owned(), "b".to_owned())].into();
        let trimmed = trim(doc.clone(), &keep).unwrap();
        assert!(trimmed.emit().contains("- name: b") && !trimmed.emit().contains("- name: a"));
        assert!(trim(doc, &BTreeSet::new()).is_none());
    }
}
