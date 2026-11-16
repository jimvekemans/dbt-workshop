//! Adding tables to sources: `vdp sources add db.table` and `vdp sources add --missing`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::catalog::Catalog;
use crate::editor::{self, TableEntry};
use crate::index::SourceIndex;
use crate::{SourceTarget, SourcesError, SourcesSettings, generated, similar};

/// `source('<source>', '<table>')`, or `database.table` on the command line.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AddRequest {
    pub source: String,
    pub table: String,
}

impl AddRequest {
    /// `database.table` (split at the first dot, as `add_source.py` did).
    pub fn parse(text: &str) -> Result<Self, SourcesError> {
        match text.split_once('.') {
            Some((source, table)) if !source.is_empty() && !table.is_empty() => Ok(Self {
                source: source.to_owned(),
                table: table.to_owned(),
            }),
            _ => Err(SourcesError::Invalid(format!(
                "`{text}` is not database.table"
            ))),
        }
    }
}

impl fmt::Display for AddRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.source, self.table)
    }
}

/// What happened to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// Inserted into a hand-written file, or added to a generated one.
    Added { file: PathBuf, columns: usize },
    /// A generated file already had it; its columns were refreshed (the Python behaviour).
    Refreshed { file: PathBuf },
    /// A hand-written file already declares it; left alone.
    AlreadyDeclared { file: PathBuf },
    /// The database has no such table.
    NotInDatabase {
        database: String,
        suggestions: Vec<String>,
    },
    /// The declaring file can't be edited safely (flow style, Jinja database, ...).
    Unsupported { file: PathBuf, reason: String },
}

impl AddOutcome {
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::NotInDatabase { .. } | Self::Unsupported { .. })
    }
}

/// Adds the requested tables. With `dry_run`, decides everything (including the database lookups) but writes
/// nothing.
pub fn add(
    project_root: &Path,
    model_paths: &[PathBuf],
    settings: &SourcesSettings,
    catalog: &dyn Catalog,
    requests: &[AddRequest],
    dry_run: bool,
) -> Result<Vec<(AddRequest, AddOutcome)>, SourcesError> {
    let job = Job {
        generated_dir: project_root.join(&settings.generated_dir),
        settings,
        catalog,
        dry_run,
    };
    let mut results = Vec::new();
    // Group per source so each file is read and written once per source.
    let mut by_source: BTreeMap<&str, Vec<&AddRequest>> = BTreeMap::new();
    for request in requests {
        by_source.entry(&request.source).or_default().push(request);
    }
    for (source, requests) in by_source {
        let index = SourceIndex::scan(model_paths)?;
        results.extend(job.add_to_source(&index, source, &requests)?);
    }
    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

type Results = Vec<(AddRequest, AddOutcome)>;

struct Job<'a> {
    generated_dir: PathBuf,
    settings: &'a SourcesSettings,
    catalog: &'a dyn Catalog,
    dry_run: bool,
}

impl Job<'_> {
    fn add_to_source(
        &self,
        index: &SourceIndex,
        source: &str,
        requests: &[&AddRequest],
    ) -> Result<Results, SourcesError> {
        // `source` names a source (dbt's undefined sources, or typed), or is a physical database typed as
        // `dev_landing.table`, written as the source `landing` under `[sources.database_prefix]`.
        let target = self.settings.target_for(source);
        let decl = index.find(source).or_else(|| index.find(&target.name));
        let (database, name) = match decl {
            None => (source.to_owned(), target.name.clone()),
            Some(decl) => {
                let Some(database) = decl.physical_database_with(self.settings) else {
                    let reason = format!(
                        "the database of source `{}` is computed by Jinja; add the table by hand, or map it with \
                         [sources.database_prefix]",
                        decl.name
                    );
                    return Ok(requests
                        .iter()
                        .map(|r| {
                            let outcome = AddOutcome::Unsupported {
                                file: decl.file.clone(),
                                reason: reason.clone(),
                            };
                            ((*r).clone(), outcome)
                        })
                        .collect());
                };
                (database, decl.name.clone())
            }
        };
        let file = decl.map_or_else(
            || generated::path(&self.generated_dir, &target.name),
            |d| d.file.clone(),
        );
        let hand_written = decl.is_some() && !generated::is_generated(&self.generated_dir, &file);

        let mut results = Vec::new();
        let mut found = Vec::new();
        for (request, columns) in self.lookup(&database, requests, &mut results)? {
            if hand_written && decl.is_some_and(|d| d.declares(&request.table)) {
                results.push((
                    request.clone(),
                    AddOutcome::AlreadyDeclared { file: file.clone() },
                ));
            } else {
                found.push(TableEntry {
                    name: request.table.clone(),
                    columns,
                });
            }
        }
        if !found.is_empty() {
            if hand_written {
                results.extend(self.insert(&file, &name, source, &found)?);
            } else {
                let target = SourceTarget {
                    name,
                    database: self.settings.target_for(&database).database,
                };
                results.extend(self.upsert(&file, &target, source, &found)?);
            }
        }
        Ok(results)
    }

    /// The columns of each requested table; tables the database lacks are reported in `missing`.
    fn lookup<'r>(
        &self,
        database: &str,
        requests: &[&'r AddRequest],
        missing: &mut Results,
    ) -> Result<Vec<(&'r AddRequest, Vec<String>)>, SourcesError> {
        let names: Vec<String> = requests.iter().map(|r| r.table.clone()).collect();
        let columns = self.catalog.columns(database, Some(&names))?;
        let mut all_tables: Option<Vec<String>> = None;
        let mut found = Vec::new();
        for request in requests {
            let table_columns: Vec<String> = columns
                .iter()
                .filter(|c| c.table == request.table)
                .map(|c| c.column.clone())
                .collect();
            if table_columns.is_empty() {
                if all_tables.is_none() {
                    all_tables = Some(self.catalog.tables(database)?);
                }
                let suggestions =
                    similar::closest(&request.table, all_tables.as_deref().unwrap_or_default());
                missing.push((
                    (*request).clone(),
                    AddOutcome::NotInDatabase {
                        database: database.to_owned(),
                        suggestions,
                    },
                ));
            } else {
                found.push((*request, table_columns));
            }
        }
        Ok(found)
    }

    /// Hand-written file: text insertion into the source `name`, one table at a time, one write. `requested` is
    /// the source as requested (reported back).
    fn insert(
        &self,
        file: &Path,
        name: &str,
        requested: &str,
        tables: &[TableEntry],
    ) -> Result<Results, SourcesError> {
        let io = |e| SourcesError::Io {
            path: file.to_owned(),
            source: e,
        };
        let mut text = std::fs::read_to_string(file).map_err(io)?;
        let mut results = Vec::new();
        for entry in tables {
            let outcome =
                match editor::insert_table(&text, name, entry, self.settings.include_columns) {
                    Ok(next) => {
                        text = next;
                        AddOutcome::Added {
                            file: file.to_owned(),
                            columns: entry.columns.len(),
                        }
                    }
                    Err(reason) => AddOutcome::Unsupported {
                        file: file.to_owned(),
                        reason,
                    },
                };
            results.push((request_for(requested, entry), outcome));
        }
        if !self.dry_run && results.iter().any(|(_, o)| !o.is_failure()) {
            std::fs::write(file, &text).map_err(io)?;
        }
        Ok(results)
    }

    /// Generated file: model update and rewrite (Python behaviour, metadata kept).
    fn upsert(
        &self,
        file: &Path,
        target: &SourceTarget,
        requested: &str,
        tables: &[TableEntry],
    ) -> Result<Results, SourcesError> {
        let current = generated::load(file)?;
        let (doc, added) =
            generated::upsert_target(current, target, tables, self.settings.include_columns);
        if !self.dry_run {
            generated::write(file, &doc)?;
        }
        Ok(tables
            .iter()
            .map(|entry| {
                let outcome = if added.contains(&entry.name) {
                    AddOutcome::Added {
                        file: file.to_owned(),
                        columns: entry.columns.len(),
                    }
                } else {
                    AddOutcome::Refreshed {
                        file: file.to_owned(),
                    }
                };
                (request_for(requested, entry), outcome)
            })
            .collect())
    }
}

fn request_for(source: &str, entry: &TableEntry) -> AddRequest {
    AddRequest {
        source: source.to_owned(),
        table: entry.name.clone(),
    }
}
