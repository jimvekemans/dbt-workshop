//! dbt source maintenance against the SingleStore catalog.
//!
//! - [`add()`]: `vdp sources add db.table ...` and `vdp sources add --missing` (the sources dbt reports as
//!   undefined, dbt1048). A source declared in a hand-written file gets the table inserted as text
//!   ([`editor`]); otherwise the generated `models/sources/sources_<db>.yml` is updated ([`generated`]).
//! - [`rewrite`]: `vdp sources rewrite`, regenerate every configured database and trim to what is referenced.
//! - [`Catalog`]: the database side, over `information_schema` with bound parameters.

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

pub use add::{AddOutcome, AddRequest, add};
pub use catalog::{Catalog, CatalogColumn, SqlCatalog};
pub use doc::Doc;
pub use error::SourcesError;
pub use index::{SourceDecl, SourceIndex};
pub use referenced::referenced_sources;
pub use settings::{DatabasePrefix, SourceTarget, SourcesSettings};
