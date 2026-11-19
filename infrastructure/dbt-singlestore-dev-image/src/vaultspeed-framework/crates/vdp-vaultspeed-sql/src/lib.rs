//! SQL text handling for `VaultSpeed` artifacts.
//!
//! The pipeline for one file is: [`StatementSplitter`] → [`SqlPreprocessor`]s →
//! [`SqlRewriter`] → [`classify`] → fingerprints, assembled by [`ArtifactPipeline`].
//! Everything here is pure: text in, domain values out.

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
