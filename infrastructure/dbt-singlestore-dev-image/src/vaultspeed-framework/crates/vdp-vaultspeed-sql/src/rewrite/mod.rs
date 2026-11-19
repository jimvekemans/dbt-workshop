//! Regex rewrite rules (`QUERY_MODIFICATIONS.json`) applied to `VaultSpeed` SQL.

mod python;
mod rule;
mod rule_set;

use std::fmt::Debug;

pub use rule::RewriteRule;
pub use rule_set::RuleSet;

/// Result of rewriting one statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub text: String,
    /// Ids of rules that changed the text, in application order.
    pub applied: Vec<String>,
}

/// Turns `VaultSpeed`'s SQL into the SQL this platform needs.
pub trait SqlRewriter: Send + Sync + Debug {
    /// Rewrites one statement, reporting which rules changed it.
    fn rewrite(&self, sql: &str) -> Rewritten;
}

/// Leaves SQL untouched (PML is hand-written for the target platform).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRewrite;

impl SqlRewriter for NoRewrite {
    fn rewrite(&self, sql: &str) -> Rewritten {
        Rewritten {
            text: sql.to_owned(),
            applied: Vec::new(),
        }
    }
}
