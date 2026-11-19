//! The DAG transformers. Each one locates its target in the syntax tree and is a no-op
//! when its change is already present.

mod default_args;
mod import;
mod outlets;
mod source;

use std::fmt::Debug;

pub use default_args::DefaultArgsEntry;
pub use import::EnsureImport;
pub use outlets::{LoadSuccessOutlet, MappingTaskOutlets};
pub use source::SourceResolver;

use crate::{DagContext, DagDocument, DagError, TextEdit};

/// One alteration of a `VaultSpeed` DAG.
pub trait DagTransformer: Send + Sync + Debug {
    /// A stable identifier for the transformer, reported when it changed a DAG.
    fn id(&self) -> &str;

    /// Edits to apply; empty when the change is already present.
    ///
    /// # Errors
    ///
    /// [`DagError::NotApplicable`] when the DAG does not have the structure the transformer needs; [`DagError::UnknownSource`]
    /// when it needs the DAG's source system and cannot determine it.
    fn edits(&self, doc: &DagDocument, ctx: &DagContext) -> Result<Vec<TextEdit>, DagError>;
}
