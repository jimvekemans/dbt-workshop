//! Text transformations applied before rewrite rules.

use std::borrow::Cow;
use std::fmt::Debug;

use regex::Regex;

use crate::lexer::tokenize;

/// A deterministic text transformation applied to each statement before rewrite rules.
pub trait SqlPreprocessor: Send + Sync + Debug {
    fn id(&self) -> &str;
    fn apply<'a>(&self, sql: &'a str) -> Cow<'a, str>;
}

/// Removes comments, unless the statement matches `keep_when`.
///
/// This reproduces the Python framework's `sqlparse.format(strip_comments=...)` step for
/// DDL: `VaultSpeed` puts `SHARD KEY`/`SORT KEY` clauses in comments, and the DDL rewrite
/// rules move them into the table definition, so those statements keep their comments.
#[derive(Debug, Clone)]
pub struct StripComments {
    keep_when: Option<Regex>,
}

impl StripComments {
    pub const fn always() -> Self {
        Self { keep_when: None }
    }

    pub const fn unless(keep_when: Regex) -> Self {
        Self {
            keep_when: Some(keep_when),
        }
    }

    /// The `VaultSpeed` default: keep comments when a line starts with `-- , SHARD KEY`.
    pub fn unless_shard_key_hint() -> Self {
        Self::unless(Regex::new(r"(?m)^-{2}[\s,]+SHARD KEY").expect("static regex is valid"))
    }
}

impl SqlPreprocessor for StripComments {
    fn id(&self) -> &'static str {
        "strip-comments"
    }

    fn apply<'a>(&self, sql: &'a str) -> Cow<'a, str> {
        if self.keep_when.as_ref().is_some_and(|re| re.is_match(sql)) {
            return Cow::Borrowed(sql);
        }
        let tokens = tokenize(sql);
        if !tokens.iter().any(super::lexer::Token::is_comment) {
            return Cow::Borrowed(sql);
        }
        let stripped: String = tokens
            .iter()
            .filter(|t| !t.is_comment())
            .map(|t| t.text)
            .collect();
        Cow::Owned(stripped.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_but_not_comment_like_strings() {
        let pre = StripComments::always();
        assert_eq!(
            pre.apply("/* h */\n-- START\nALTER TABLE t COMMENT '-- not a comment' -- x"),
            "ALTER TABLE t COMMENT '-- not a comment'"
        );
    }

    #[test]
    fn keeps_shard_key_hints() {
        let pre = StripComments::unless_shard_key_hint();
        let sql = "CREATE TABLE t (\n a INT\n)\n-- , SHARD KEY (`a`)\n";
        assert_eq!(pre.apply(sql), sql);
    }
}
