//! Adding tables to sources: `vdp sources add db.table` and `vdp sources add --missing`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use vdp_core::{BatchOutcome, BatchReport};

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
    ///
    /// # Errors
    ///
    /// [`SourcesError::Invalid`] when `text` has no dot or an empty database or table part.
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

/// What was done for a request that did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddSuccess {
    /// Inserted into a hand-written file, or added to a generated one.
    Added { file: PathBuf, columns: usize },
    /// A generated file already had it; its columns were refreshed (the Python behaviour).
    Refreshed { file: PathBuf },
    /// A hand-written file already declares it; left alone.
    AlreadyDeclared { file: PathBuf },
}

/// Why a request could not be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddFailure {
    /// The database has no such table.
    NotInDatabase {
        database: String,
        suggestions: Vec<String>,
    },
    /// The declaring file can't be edited safely (flow style, Jinja database, ...).
    Unsupported { file: PathBuf, reason: String },
}

/// What happened to one request.
pub type AddOutcome = BatchOutcome<AddSuccess, AddFailure>;

/// What happened to every request of [`add`], ordered by request. A request that fails does not stop the others.
pub type AddReport = BatchReport<AddRequest, AddSuccess, AddFailure>;

/// Inputs for [`add`].
#[derive(Clone, Copy)]
pub struct AddOptions<'a> {
    pub project_root: &'a Path,
    pub model_paths: &'a [PathBuf],
    pub settings: &'a SourcesSettings,
    pub catalog: &'a dyn Catalog,
    pub requests: &'a [AddRequest],
    pub dry_run: bool,
}

impl std::fmt::Debug for AddOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddOptions")
            .field("project_root", &self.project_root)
            .field("model_paths", &self.model_paths)
            .field("settings", &self.settings)
            .field("catalog", &"<catalog>")
            .field("requests", &self.requests)
            .field("dry_run", &self.dry_run)
            .finish()
    }
}

/// Adds the requested tables. With `dry_run`, decides everything (including the database lookups) but writes
/// nothing.
///
/// # Errors
///
/// [`SourcesError::Yaml`] when a source file under `model_paths` cannot be parsed; [`SourcesError::Io`] when a file
/// cannot be read or written; [`SourcesError::Catalog`] when the database lookup fails. A table that cannot be added to a
/// hand-written file is not an error: it is reported as a failure in the [`AddReport`].
pub fn add(options: AddOptions<'_>) -> Result<AddReport, SourcesError> {
    let job = Job {
        generated_dir: options.project_root.join(&options.settings.generated_dir),
        settings: options.settings,
        catalog: options.catalog,
        dry_run: options.dry_run,
    };
    let mut results = Vec::new();
    // Group per source so each file is read and written once per source.
    let mut by_source: BTreeMap<&str, Vec<&AddRequest>> = BTreeMap::new();
    for request in options.requests {
        by_source.entry(&request.source).or_default().push(request);
    }
    for (source, requests) in by_source {
        let index = SourceIndex::scan(options.model_paths)?;
        results.extend(job.add_to_source(&index, source, &requests)?);
    }
    results.sort_by(|a, b| a.0.cmp(&b.0));
    let mut report = AddReport::with_capacity(results.len());
    for (request, outcome) in results {
        report.push(request, outcome);
    }
    Ok(report)
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
                            let outcome = BatchOutcome::Failed(AddFailure::Unsupported {
                                file: decl.file.clone(),
                                reason: reason.clone(),
                            });
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
                    BatchOutcome::Succeeded(AddSuccess::AlreadyDeclared { file: file.clone() }),
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
                    BatchOutcome::Failed(AddFailure::NotInDatabase {
                        database: database.to_owned(),
                        suggestions,
                    }),
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
            let outcome = match editor::insert_table(
                &text,
                editor::InsertTable {
                    source: name,
                    entry,
                    include_columns: self.settings.include_columns,
                },
            ) {
                Ok(next) => {
                    text = next;
                    BatchOutcome::Succeeded(AddSuccess::Added {
                        file: file.to_owned(),
                        columns: entry.columns.len(),
                    })
                }
                Err(reason) => BatchOutcome::Failed(AddFailure::Unsupported {
                    file: file.to_owned(),
                    reason,
                }),
            };
            results.push((request_for(requested, entry), outcome));
        }
        if !self.dry_run && results.iter().any(|(_, o)| !o.is_failed()) {
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
        let (doc, added) = generated::upsert_target(
            current,
            target,
            generated::TablesUpdate {
                tables,
                include_columns: self.settings.include_columns,
            },
        );
        if !self.dry_run {
            generated::write(file, &doc)?;
        }
        Ok(tables
            .iter()
            .map(|entry| {
                let outcome = if added.contains(&entry.name) {
                    BatchOutcome::Succeeded(AddSuccess::Added {
                        file: file.to_owned(),
                        columns: entry.columns.len(),
                    })
                } else {
                    BatchOutcome::Succeeded(AddSuccess::Refreshed {
                        file: file.to_owned(),
                    })
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

#[cfg(test)]
mod tests {
    use vdp_core::Outcome;
    use vdp_db::DbError;

    use super::*;
    use crate::catalog::CatalogColumn;

    /// `hand_db` has `existing`, `fresh` and `other`; `gen` has `t1`.
    #[derive(Debug)]
    struct FakeCatalog;

    impl Catalog for FakeCatalog {
        fn tables(&self, database: &str) -> Result<Vec<String>, DbError> {
            Ok(match database {
                "hand_db" => vec!["existing".into(), "fresh".into(), "other".into()],
                "gen" => vec!["t1".into()],
                _ => Vec::new(),
            })
        }

        fn columns(
            &self,
            database: &str,
            tables: Option<&[String]>,
        ) -> Result<Vec<CatalogColumn>, DbError> {
            Ok(self
                .tables(database)?
                .into_iter()
                .filter(|t| tables.is_none_or(|wanted| wanted.contains(t)))
                .map(|table| CatalogColumn {
                    table,
                    column: "id".into(),
                })
                .collect())
        }
    }

    fn request(text: &str) -> AddRequest {
        AddRequest::parse(text).unwrap()
    }

    #[test]
    fn add_reports_every_request_in_order_and_failures_do_not_stop_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        std::fs::create_dir_all(&models).unwrap();
        let hand = models.join("hand.yml");
        std::fs::write(
            &hand,
            "version: 2\nsources:\n  - name: hand\n    database: hand_db\n    tables:\n      - name: existing\n",
        )
        .unwrap();
        let requests = [
            request("hand.missing"),
            request("hand.fresh"),
            request("gen.t1"),
            request("hand.existing"),
        ];
        let settings = SourcesSettings::default();

        let report = add(AddOptions {
            project_root: dir.path(),
            model_paths: std::slice::from_ref(&models),
            settings: &settings,
            catalog: &FakeCatalog,
            requests: &requests,
            dry_run: true,
        })
        .unwrap();

        let keys: Vec<String> = report.records().iter().map(|r| r.key.to_string()).collect();
        assert_eq!(
            keys,
            ["gen.t1", "hand.existing", "hand.fresh", "hand.missing"]
        );
        let outcomes: Vec<&AddOutcome> = report.records().iter().map(|r| &r.outcome).collect();
        assert!(matches!(
            outcomes[0],
            BatchOutcome::Succeeded(AddSuccess::Added { columns: 1, .. })
        ));
        assert_eq!(
            outcomes[1],
            &BatchOutcome::Succeeded(AddSuccess::AlreadyDeclared { file: hand.clone() })
        );
        assert_eq!(
            outcomes[2],
            &BatchOutcome::Succeeded(AddSuccess::Added {
                file: hand,
                columns: 1
            })
        );
        assert_eq!(
            outcomes[3],
            &BatchOutcome::Failed(AddFailure::NotInDatabase {
                database: "hand_db".into(),
                suggestions: Vec::new(),
            })
        );
        assert_eq!(report.counts().succeeded, 3);
        assert_eq!(report.counts().failed, 1);
        assert_eq!(report.outcome(), Outcome::Failures);
    }
}
