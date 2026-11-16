use std::fmt::Debug;
use std::sync::Arc;

use vdp_vaultspeed_domain::{GenerationKind, SourceKind, StatementGroup};

use crate::sources::{ArtifactLoader, DiscoveredSource, SourceError, SourceFile};

/// Airflow files of a source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DagFiles {
    pub dags: Vec<SourceFile>,
    pub metadata: Vec<SourceFile>,
}

impl DagFiles {
    pub fn is_empty(&self) -> bool {
        self.dags.is_empty() && self.metadata.is_empty()
    }
}

/// Everything a source asks the engine to do.
#[derive(Debug, Clone, Default)]
pub struct SourceContent {
    pub groups: Vec<StatementGroup>,
    pub dags: DagFiles,
}

/// Interprets the files of a source.
pub trait SourceHandler: Send + Sync + Debug {
    fn supports(&self, kind: &SourceKind) -> bool;
    fn load(
        &self,
        source: &DiscoveredSource,
        files: &[SourceFile],
    ) -> Result<SourceContent, SourceError>;
}

/// Staged PML scripts: one group per file.
#[derive(Debug, Clone)]
pub struct PmlHandler {
    loader: Arc<ArtifactLoader>,
}

impl PmlHandler {
    pub const fn new(loader: Arc<ArtifactLoader>) -> Self {
        Self { loader }
    }
}

impl SourceHandler for PmlHandler {
    fn supports(&self, kind: &SourceKind) -> bool {
        matches!(kind, SourceKind::Pml)
    }

    fn load(
        &self,
        source: &DiscoveredSource,
        files: &[SourceFile],
    ) -> Result<SourceContent, SourceError> {
        Ok(SourceContent {
            groups: self.loader.load(&source.kind, None, "", files)?,
            dags: DagFiles::default(),
        })
    }
}

/// A SQL section of a generation zip: files below `root` loaded by `loader`.
#[derive(Debug, Clone)]
struct SqlSection {
    root: String,
    loader: Arc<ArtifactLoader>,
}

/// One generation kind, composed from SQL sections and an optional DAG section.
///
/// The composition root configures, e.g., `COMP` as `DDL/` with the DDL loader plus
/// `ETL/` with the ETL loader, and `FMC` as just a DAG section.
#[derive(Debug, Clone)]
pub struct GenerationHandler {
    kind: GenerationKind,
    sections: Vec<SqlSection>,
    dag_root: Option<String>,
}

impl GenerationHandler {
    pub const fn new(kind: GenerationKind) -> Self {
        Self {
            kind,
            sections: Vec::new(),
            dag_root: None,
        }
    }

    #[must_use]
    pub fn with_sql(mut self, root: &str, loader: Arc<ArtifactLoader>) -> Self {
        self.sections.push(SqlSection {
            root: root.to_owned(),
            loader,
        });
        self
    }

    /// Collects top-level `.py` (DAGs) and `.json` (metadata) files below `root`.
    #[must_use]
    pub fn with_dags(mut self, root: &str) -> Self {
        self.dag_root = Some(root.to_owned());
        self
    }

    fn collect_dags(root: &str, files: &[SourceFile]) -> DagFiles {
        let mut dags = DagFiles::default();
        for file in files {
            let Some(rel) = file.path.strip_prefix(root) else {
                continue;
            };
            if rel.components().count() != 1 {
                continue;
            }
            let renamed = SourceFile::new(rel.clone(), file.content.clone());
            if rel.has_extension("py") {
                dags.dags.push(renamed);
            } else if rel.has_extension("json") {
                dags.metadata.push(renamed);
            } else {
                tracing::warn!(file = %file.path, "ignoring non-Airflow file in DAG section");
            }
        }
        dags
    }
}

impl SourceHandler for GenerationHandler {
    fn supports(&self, kind: &SourceKind) -> bool {
        matches!(kind, SourceKind::Generation(name) if name.kind() == self.kind)
    }

    fn load(
        &self,
        source: &DiscoveredSource,
        files: &[SourceFile],
    ) -> Result<SourceContent, SourceError> {
        let SourceKind::Generation(name) = &source.kind else {
            return Err(SourceError::Unsupported(source.kind.to_string()));
        };
        let mut content = SourceContent::default();
        for section in &self.sections {
            content.groups.extend(section.loader.load(
                &source.kind,
                Some(name.id()),
                &section.root,
                files,
            )?);
        }
        if let Some(root) = &self.dag_root {
            content.dags = Self::collect_dags(root, files);
        }
        Ok(content)
    }
}
