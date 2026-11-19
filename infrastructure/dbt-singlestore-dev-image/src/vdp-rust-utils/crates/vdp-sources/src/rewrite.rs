//! `vdp sources rewrite`: regenerate the generated files from the catalog, then keep only what the project
//! references (`update_source_files.py`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use vdp_dbt::artifacts::Manifest;

use crate::catalog::Catalog;
use crate::editor::TableEntry;
use crate::index::SourceIndex;
use crate::{SourcesError, SourcesSettings, generated};

pub use vdp_core::FileStatus;

/// What the refresh did per database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refreshed {
    Written {
        database: String,
        file: PathBuf,
        tables: usize,
        status: FileStatus,
    },
    /// A hand-written file declares a source with this name; a generated file would duplicate it.
    SkippedHandWritten {
        database: String,
        file: PathBuf,
    },
    Empty {
        database: String,
    },
}

/// Inputs for [`refresh`].
#[derive(Clone, Copy)]
pub struct RefreshOptions<'a> {
    pub project_root: &'a Path,
    pub model_paths: &'a [PathBuf],
    pub settings: &'a SourcesSettings,
    pub catalog: &'a dyn Catalog,
    pub databases: &'a [String],
    pub dry_run: bool,
}

impl std::fmt::Debug for RefreshOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshOptions")
            .field("project_root", &self.project_root)
            .field("model_paths", &self.model_paths)
            .field("settings", &self.settings)
            .field("catalog", &"<catalog>")
            .field("databases", &self.databases)
            .field("dry_run", &self.dry_run)
            .finish()
    }
}

/// Step 1: every table of every configured database into its generated file (named after the source:
/// `sources_landing.yml` for `dev_landing` under `[sources.database_prefix]`), skipping writes when `dry_run` is
/// true.
///
/// # Errors
///
/// [`SourcesError::Catalog`] when the catalog cannot be read; [`SourcesError::Io`] when a file cannot be read or written;
/// [`SourcesError::Yaml`] when an existing generated file cannot be parsed.
pub fn refresh(options: RefreshOptions<'_>) -> Result<Vec<Refreshed>, SourcesError> {
    let RefreshOptions {
        project_root,
        model_paths,
        settings,
        catalog,
        databases,
        dry_run,
    } = options;
    let generated_dir = project_root.join(&settings.generated_dir);
    let index = SourceIndex::scan(model_paths)?;
    let mut report = Vec::new();
    for database in databases {
        let target = settings.target_for(database);
        if let Some(decl) = index.find(&target.name)
            && !generated::is_generated(&generated_dir, &decl.file)
        {
            report.push(Refreshed::SkippedHandWritten {
                database: database.clone(),
                file: decl.file.clone(),
            });
            continue;
        }
        let columns = catalog.columns(database, None)?;
        if columns.is_empty() {
            report.push(Refreshed::Empty {
                database: database.clone(),
            });
            continue;
        }
        let mut tables: Vec<TableEntry> = Vec::new();
        for column in columns {
            match tables.last_mut() {
                Some(last) if last.name == column.table => last.columns.push(column.column),
                _ => tables.push(TableEntry {
                    name: column.table,
                    columns: vec![column.column],
                }),
            }
        }
        let file = generated::path(&generated_dir, &target.name);
        let doc = generated::replace_all_target(
            generated::load(&file)?,
            &target,
            generated::TablesUpdate {
                tables: &tables,
                include_columns: settings.include_columns,
            },
        );
        let rendered = doc.emit();
        let status = FileStatus::diff(&file, &rendered).map_err(|source| SourcesError::Io {
            path: file.clone(),
            source,
        })?;
        if !dry_run {
            generated::write(&file, &doc)?;
        }
        report.push(Refreshed::Written {
            database: database.clone(),
            file,
            tables: tables.len(),
            status,
        });
    }
    Ok(report)
}

/// Inputs for [`trim`].
#[derive(Debug, Clone, Copy)]
pub struct TrimOptions<'a> {
    pub project_root: &'a Path,
    pub settings: &'a SourcesSettings,
    pub manifest: &'a Manifest,
    pub databases: &'a [String],
}

/// What trimming did per generated file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trimmed {
    Kept { file: PathBuf, tables: usize },
    Deleted { file: PathBuf },
}

/// Step 3: keep only referenced tables in the generated files of `databases`; delete files left empty.
///
/// # Errors
///
/// [`SourcesError::Io`] when a generated file cannot be read, written or deleted; [`SourcesError::Yaml`] when it cannot be
/// parsed.
pub fn trim(options: TrimOptions<'_>) -> Result<Vec<Trimmed>, SourcesError> {
    let TrimOptions {
        project_root,
        settings,
        manifest,
        databases,
    } = options;
    let generated_dir = project_root.join(&settings.generated_dir);
    let referenced: BTreeSet<(String, String)> = crate::referenced_sources(manifest);
    let mut report = Vec::new();
    for database in databases {
        let name = settings.target_for(database).name;
        let file = generated::path(&generated_dir, &name);
        let Some(doc) = generated::load(&file)? else {
            continue;
        };
        match generated::trim(doc, &referenced) {
            Some(doc) => {
                let tables = referenced.iter().filter(|(s, _)| *s == name).count();
                generated::write(&file, &doc)?;
                report.push(Trimmed::Kept { file, tables });
            }
            None => {
                std::fs::remove_file(&file).map_err(|source| SourcesError::Io {
                    path: file.clone(),
                    source,
                })?;
                report.push(Trimmed::Deleted { file });
            }
        }
    }
    Ok(report)
}
