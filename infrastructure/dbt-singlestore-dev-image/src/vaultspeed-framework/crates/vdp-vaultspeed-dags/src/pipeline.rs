use std::sync::Arc;

use crate::{DagContext, DagDocument, DagError, DagTransformer, DagValidator};

/// Result of rendering one DAG.
#[derive(Debug)]
pub struct RenderedDag {
    pub document: DagDocument,
    /// Ids of transformers that changed the document.
    pub applied: Vec<String>,
}

/// Applies transformers in order, re-parsing after each, then validates.
#[derive(Debug, Clone)]
pub struct DagAlterationPipeline {
    transformers: Vec<Arc<dyn DagTransformer>>,
    validators: Vec<Arc<dyn DagValidator>>,
}

impl DagAlterationPipeline {
    /// Creates a pipeline that applies `transformers` in order and then checks the result with `validators`.
    pub fn new(
        transformers: Vec<Arc<dyn DagTransformer>>,
        validators: Vec<Arc<dyn DagValidator>>,
    ) -> Self {
        Self {
            transformers,
            validators,
        }
    }

    /// Applies every transformer to `original`, re-parsing after each, then runs the validators.
    ///
    /// # Errors
    ///
    /// Whatever a transformer or validator returns ([`DagError::NotApplicable`], [`DagError::UnknownSource`],
    /// [`DagError::Invalid`]); [`DagError::OverlappingEdits`] when one transformer's edits overlap; [`DagError::Unparseable`]
    /// when an intermediate result is not Python.
    pub fn render(&self, original: DagDocument, ctx: &DagContext) -> Result<RenderedDag, DagError> {
        let mut document = original;
        let mut applied = Vec::new();
        for transformer in &self.transformers {
            let edits = transformer.edits(&document, ctx)?;
            if edits.is_empty() {
                continue;
            }
            document = document.with_edits(edits, transformer.id())?;
            applied.push(transformer.id().to_owned());
        }
        for validator in &self.validators {
            validator.validate(&document)?;
        }
        Ok(RenderedDag { document, applied })
    }
}
