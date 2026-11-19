use std::sync::Arc;

use crate::sources::DagFiles;
use similar::TextDiff;
use vdp_vaultspeed_dags::{
    DagAlterationPipeline, DagContext, DagDocument, DagNamer, DagPublisher, PublishBundle,
    PublishedFile,
};
use vdp_vaultspeed_domain::{DagReport, GenerationKind, RunMode, SourceKind};

/// Renders a source's DAGs in memory and publishes them with their metadata.
#[derive(Debug, Clone)]
pub struct DagStage {
    pipeline: DagAlterationPipeline,
    namer: Arc<dyn DagNamer>,
    publisher: Arc<dyn DagPublisher>,
    context: DagContext,
    publish_kinds: Vec<GenerationKind>,
}

impl DagStage {
    pub fn new(
        pipeline: DagAlterationPipeline,
        namer: Arc<dyn DagNamer>,
        publisher: Arc<dyn DagPublisher>,
        context: DagContext,
        publish_kinds: Vec<GenerationKind>,
    ) -> Self {
        Self {
            pipeline,
            namer,
            publisher,
            context,
            publish_kinds,
        }
    }

    pub fn handles(&self, source: &SourceKind) -> bool {
        matches!(source, SourceKind::Generation(name) if self.publish_kinds.contains(&name.kind()))
    }

    /// Renders every DAG; publishes the bundle only when all of them rendered, so
    /// Airflow never receives half a release. Returns per-DAG reports and an error
    /// message when publication failed.
    pub fn process(&self, files: &DagFiles, mode: RunMode) -> (Vec<DagReport>, Option<String>) {
        let mut reports = Vec::with_capacity(files.dags.len());
        let mut bundle = PublishBundle {
            metadata: files
                .metadata
                .iter()
                .map(|f| PublishedFile {
                    name: f.path.file_name().to_owned(),
                    content: f.content.clone(),
                })
                .collect(),
            dags: Vec::new(),
        };
        for dag in &files.dags {
            let source_file = dag.path.file_name().to_owned();
            let target_file = self.namer.target_name(&source_file);
            let rendered = DagDocument::parse(&source_file, &dag.content)
                .and_then(|doc| self.pipeline.render(doc, &self.context));
            match rendered {
                Ok(rendered) => {
                    let text = rendered.document.text().to_owned();
                    let diff = TextDiff::from_lines(dag.content.as_str(), text.as_str())
                        .unified_diff()
                        .header(&source_file, &target_file)
                        .to_string();
                    reports.push(DagReport {
                        source_file,
                        target_file: target_file.clone(),
                        transformations: rendered.applied,
                        published: false,
                        diff: Some(diff),
                        error: None,
                    });
                    bundle.dags.push(PublishedFile {
                        name: target_file,
                        content: text,
                    });
                }
                Err(error) => reports.push(DagReport {
                    source_file,
                    target_file,
                    transformations: Vec::new(),
                    published: false,
                    diff: None,
                    error: Some(error.to_string()),
                }),
            }
        }

        if reports.iter().any(|r| r.error.is_some()) {
            return (
                reports,
                Some("not publishing: at least one DAG failed to render".into()),
            );
        }
        if mode.is_preview() || bundle.dags.is_empty() && bundle.metadata.is_empty() {
            return (reports, None);
        }
        match self.publisher.publish(&bundle) {
            Ok(()) => {
                for report in &mut reports {
                    report.published = true;
                }
                tracing::info!(dags = bundle.dags.len(), target = %self.publisher.describe(), "published DAGs");
                (reports, None)
            }
            Err(error) => (
                reports,
                Some(format!(
                    "publishing to {} failed: {error}",
                    self.publisher.describe()
                )),
            ),
        }
    }
}
