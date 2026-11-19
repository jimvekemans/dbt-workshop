//! dbt source maintenance against the SingleStore catalog.
//!
//! A domain module over the [`Catalog`] port (SingleStore's `information_schema`) and the project's YAML files. `vdp
//! sources` in `vdp-cli` runs it, and the `dbt` wrapper suggests it when dbt reports an undefined source.
//!
//! # Responsibilities
//!
//! - [`add()`]: `vdp sources add db.table ...` and `vdp sources add --missing` (the sources dbt reports as undefined,
//!   dbt1048). A source declared in a hand-written file gets the table inserted as text ([`editor`]); otherwise the
//!   generated `models/sources/sources_<db>.yml` is updated ([`generated`]).
//! - [`rewrite`]: `vdp sources rewrite`, regenerate every configured database and trim to what is referenced.
//! - [`Catalog`]: the database side, over `information_schema` with bound parameters ([`SqlCatalog`]).
//! - [`SourceIndex`]: the sources declared in the project's YAML, read without regular expressions.
//!
//! # Example
//!
//! ```
//! use vdp_sources::AddRequest;
//!
//! // `database.table`, split at the first dot.
//! let request = AddRequest::parse("landing.orders")?;
//! assert_eq!((request.source.as_str(), request.table.as_str()), ("landing", "orders"));
//! assert!(AddRequest::parse("orders").is_err());
//! # Ok::<(), vdp_sources::SourcesError>(())
//! ```
//!
//! Adding the table to the project (reads the catalog and writes YAML, so it is only compiled here):
//!
//! ```no_run
//! use std::path::{Path, PathBuf};
//! use std::sync::Arc;
//! use vdp_db::SqlSession;
//! use vdp_sources::{AddOptions, AddRequest, SourcesSettings, SqlCatalog, add};
//!
//! fn add_orders(session: Arc<dyn SqlSession>) -> Result<(), vdp_sources::SourcesError> {
//!     let settings = SourcesSettings::default();
//!     let catalog = SqlCatalog::new(session, &settings.column_order);
//!     let requests = [AddRequest::parse("landing.orders")?];
//!     let report = add(AddOptions {
//!         project_root: Path::new("."),
//!         model_paths: &[PathBuf::from("models")],
//!         settings: &settings,
//!         catalog: &catalog,
//!         requests: &requests,
//!         dry_run: true,
//!     })?;
//!     println!("{} tables", report.len());
//!     Ok(())
//! }
//! ```

pub mod add;
mod catalog;
mod doc;
pub mod editor;
mod error;
pub mod generated;
mod index;
mod referenced;
pub mod rewrite;
mod settings;
mod similar;
mod tree;

pub use add::{AddFailure, AddOptions, AddOutcome, AddReport, AddRequest, AddSuccess, add};
pub use catalog::{Catalog, CatalogColumn, SqlCatalog};
pub use doc::Doc;
pub use error::SourcesError;
pub use index::{SourceDecl, SourceIndex};
pub use referenced::referenced_sources;
pub use settings::{DatabasePrefix, SourceTarget, SourcesSettings};
