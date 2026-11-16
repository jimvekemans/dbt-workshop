//! `VaultSpeed` Airflow DAG handling.
//!
//! A DAG is parsed into a [`DagDocument`] (tree-sitter Python AST + source text). Each
//! [`DagTransformer`] locates its target through the AST and returns span-based
//! [`TextEdit`]s; the [`DagAlterationPipeline`] applies them one transformer at a time,
//! re-parsing in between, and validates the result. No regular expression ever touches
//! Python source, the original file is never mutated, and every transformer is
//! idempotent (it returns no edits when its change is already present).

mod context;
mod document;
mod edit;
mod error;
mod literal;
mod naming;
mod pipeline;
mod publish;
mod transform;
mod validate;

pub use context::DagContext;
pub use document::DagDocument;
pub use edit::TextEdit;
pub use error::DagError;
pub use literal::PyLiteral;
pub use naming::{DagNamer, VaultSpeedDagNamer};
pub use pipeline::{DagAlterationPipeline, RenderedDag};
pub use publish::{
    CompositePublisher, DagPublisher, PublishBundle, PublishedFile, RepoDirPublisher, SftpPublisher,
};
pub use transform::{
    DagTransformer, DefaultArgsEntry, EnsureImport, LoadSuccessOutlet, MappingTaskOutlets,
    SourceResolver,
};
pub use validate::{DagValidator, SyntaxValidator};
