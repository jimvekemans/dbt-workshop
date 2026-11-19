//! SQL text handling for `VaultSpeed` artifacts.
//!
//! A building block over `vdp-sql-script` (the lexer and splitters) and `vdp-vaultspeed-domain`: text in, domain values
//! out. Nothing here performs IO except reading rewrite-rule files.
//!
//! # Responsibilities
//!
//! - Split a file into statements ([`StatementSplitter`]) and preprocess them ([`SqlPreprocessor`], [`StripComments`]).
//! - Rewrite what `VaultSpeed` generates into what the platform needs ([`SqlRewriter`], [`RuleSet`] from JSON rule files).
//! - Classify a statement: what it does and which object it targets ([`classify`]).
//! - Fingerprint raw and effective text ([`fingerprint`], [`normalize`]).
//! - Assemble the pipeline for one file: [`StatementSplitter`] → [`SqlPreprocessor`]s → [`SqlRewriter`] → [`classify`] →
//!   fingerprints ([`ArtifactPipeline`]).
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use vdp_vaultspeed_sql::{ArtifactPipeline, NoRewrite, RoutineAwareSplitter};
//!
//! let pipeline = ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), Vec::new(), Arc::new(NoRewrite));
//! let statements = pipeline.statements("DROP TABLE IF EXISTS edw.x; CREATE TABLE edw.x (id INT);")?;
//! assert_eq!(statements.len(), 2);
//! assert!(statements[0].kind.is_drop());
//! assert!(statements[1].kind.is_create());
//! # Ok::<(), vdp_vaultspeed_sql::SqlError>(())
//! ```

mod classify;
mod error;
mod fingerprint;
// The lexer and splitter are shared with the database session (crate vdp-sql-script).
use vdp_sql_script::lexer;
mod pipeline;
mod preprocess;
mod rewrite;
use vdp_sql_script::split;

pub use classify::{Classification, classify, classify_in, use_database};
pub use error::SqlError;
pub use fingerprint::{fingerprint, normalize};
pub use lexer::{Token, TokenKind, tokenize};
pub use pipeline::ArtifactPipeline;
pub use preprocess::{SqlPreprocessor, StripComments};
pub use rewrite::{NoRewrite, RewriteRule, Rewritten, RuleSet, SqlRewriter};
pub use split::{RoutineAwareSplitter, StatementSplitter, WholeFileSplitter};
