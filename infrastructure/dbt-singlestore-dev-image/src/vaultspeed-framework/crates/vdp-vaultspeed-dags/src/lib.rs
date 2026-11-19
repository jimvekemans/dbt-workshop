//! `VaultSpeed` Airflow DAG handling.
//!
//! A building block of the framework: it alters the DAGs `VaultSpeed` generates and publishes them. A DAG is parsed into a
//! [`DagDocument`] (tree-sitter Python AST + source text). Each [`DagTransformer`] locates its target through the AST and
//! returns span-based [`TextEdit`]s; the [`DagAlterationPipeline`] applies them one transformer at a time, re-parsing in
//! between, and validates the result. No regular expression ever touches Python source, the original file is never
//! mutated, and every transformer is idempotent (it returns no edits when its change is already present).
//!
//! # Responsibilities
//!
//! - Parse and edit Python source safely ([`DagDocument`], [`TextEdit`]).
//! - Alter DAGs: default arguments, imports, dataset outlets ([`DefaultArgsEntry`], [`EnsureImport`],
//!   [`MappingTaskOutlets`], [`LoadSuccessOutlet`]), in a pipeline that validates its result ([`DagValidator`]).
//! - Decide the published file name ([`DagNamer`]).
//! - Publish a bundle, metadata before DAGs, to the repository and to the Airflow host ([`DagPublisher`],
//!   [`RepoDirPublisher`], [`SftpPublisher`]).
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use vdp_vaultspeed_dags::{
//!     DagAlterationPipeline, DagContext, DagDocument, EnsureImport, SyntaxValidator,
//! };
//! use vdp_vaultspeed_domain::Environment;
//!
//! let context = DagContext {
//!     environment: Environment::LDV,
//!     dataset_env: "rel1ldv".into(),
//!     database: "edw_dv_rdv".into(),
//!     singlestore_host: "singlestore-rel1ldv.example.com".into(),
//!     singlestore_port: 3306,
//! };
//! let pipeline = DagAlterationPipeline::new(
//!     vec![Arc::new(EnsureImport::new("airflow.datasets", "Dataset", "airflow"))],
//!     vec![Arc::new(SyntaxValidator)],
//! );
//!
//! let original = DagDocument::parse("dag.py", "from airflow import DAG\n")?;
//! let rendered = pipeline.render(original, &context)?;
//! assert!(rendered.document.text().contains("from airflow.datasets import Dataset"));
//! assert_eq!(rendered.applied, ["import:airflow.datasets.Dataset"]);
//! # Ok::<(), vdp_vaultspeed_dags::DagError>(())
//! ```

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
