use std::collections::HashSet;
use std::sync::Arc;

use vdp_vaultspeed_domain::{
    ArtifactKind, GenerationId, GroupId, JobInfo, JobName, Layer, OrderKey, SourceKind,
    StatementGroup,
};
use vdp_vaultspeed_sql::ArtifactPipeline;

use crate::sources::{GroupingStrategy, SourceError, SourceFile};

/// Turns the `.sql` files of one artifact kind into statement groups.
#[derive(Debug, Clone)]
pub struct ArtifactLoader {
    artifact: ArtifactKind,
    pipeline: ArtifactPipeline,
    grouping: Arc<dyn GroupingStrategy>,
}

/// Inputs for [`ArtifactLoader::load`].
#[derive(Debug, Clone, Copy)]
pub struct LoadInput<'a> {
    pub source: &'a SourceKind,
    pub generation: Option<GenerationId>,
    pub root: &'a str,
    pub files: &'a [SourceFile],
}

impl ArtifactLoader {
    /// Creates the loader for `artifact`: `pipeline` turns each file into statements and `grouping` into units.
    pub fn new(
        artifact: ArtifactKind,
        pipeline: ArtifactPipeline,
        grouping: Arc<dyn GroupingStrategy>,
    ) -> Self {
        Self {
            artifact,
            pipeline,
            grouping,
        }
    }

    /// The artifact kind the loader loads.
    pub const fn artifact(&self) -> ArtifactKind {
        self.artifact
    }

    /// Loads the `.sql` files below `input.root` (`""`, `"DDL"`, `"ETL"`). Paths in the
    /// resulting [`JobInfo`] are relative to `input.root`; group ids keep the full path.
    /// Groups identical to an earlier group of the same job are dropped, as the Python
    /// implementation did with `drop_duplicates`.
    ///
    /// # Errors
    ///
    /// [`SourceError::Sql`] when a file cannot be split into statements.
    pub fn load(&self, input: LoadInput<'_>) -> Result<Vec<StatementGroup>, SourceError> {
        let LoadInput {
            source,
            generation,
            root,
            files,
        } = input;
        let mut groups = Vec::new();
        let mut seen = HashSet::new();
        for file in files {
            let Some(rel_path) = file.path.strip_prefix(root) else {
                continue;
            };
            if !rel_path.has_extension("sql") {
                continue;
            }
            let statements =
                self.pipeline
                    .statements(&file.content)
                    .map_err(|source| SourceError::Sql {
                        path: file.path.to_string(),
                        source,
                    })?;
            let stem = rel_path.file_stem();
            let job = JobName::from_file_stem(stem);
            let info = JobInfo {
                artifact: self.artifact,
                layer: Layer::infer(&rel_path.parent(), &job),
                order: OrderKey::from_file_stem(stem, generation),
                rel_path,
                job,
            };
            for (ordinal, statements) in self.grouping.group(statements).into_iter().enumerate() {
                let group = StatementGroup {
                    id: GroupId::new(source.label(), &file.path, ordinal),
                    info: info.clone(),
                    ordinal,
                    statements,
                };
                if seen.insert((group.info.job.clone(), group.raw_fp())) {
                    groups.push(group);
                }
            }
        }
        Ok(groups)
    }
}
