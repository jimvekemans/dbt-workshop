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
    fn id(&self) -> &str;

    /// Edits to apply; empty when the change is already present.
    fn edits(&self, doc: &DagDocument, ctx: &DagContext) -> Result<Vec<TextEdit>, DagError>;
}
