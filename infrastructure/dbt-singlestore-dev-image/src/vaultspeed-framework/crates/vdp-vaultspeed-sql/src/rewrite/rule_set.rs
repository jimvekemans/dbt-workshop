use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::rule::{RewriteRule, RuleSpec};
use super::{Rewritten, SqlRewriter};
use crate::SqlError;

#[derive(Debug, Deserialize)]
struct RuleFile {
    #[serde(default)]
    modifications: Vec<RuleSpec>,
}

/// An ordered list of rules applied one after the other.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<RewriteRule>,
}

impl RuleSet {
    /// A set without rules: it rewrites nothing.
    pub const fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Parses one rule file's content; rule ids are `<file_id>#<index>`.
    ///
    /// # Errors
    ///
    /// [`SqlError::InvalidRuleFile`] when `json` is not a rule file; [`SqlError::InvalidRulePattern`] when a rule has an
    /// invalid regular expression.
    pub fn from_json(file_id: &str, json: &str, origin: &Path) -> Result<Self, SqlError> {
        let file: RuleFile =
            serde_json::from_str(json).map_err(|source| SqlError::InvalidRuleFile {
                path: origin.to_owned(),
                source,
            })?;
        let rules = file
            .modifications
            .iter()
            .enumerate()
            .map(|(index, spec)| RewriteRule::compile(format!("{file_id}#{index}"), spec))
            .collect::<Result<_, _>>()?;
        Ok(Self { rules })
    }

    /// Loads **every** `*.json` file of a directory in file-name order. A missing
    /// directory yields an empty rule set.
    ///
    /// # Errors
    ///
    /// [`SqlError::RuleIo`] when the directory or a file cannot be read; otherwise as [`RuleSet::from_json`] for each file.
    pub fn from_dir(dir: &Path) -> Result<Self, SqlError> {
        if !dir.is_dir() {
            return Ok(Self::empty());
        }
        let io = |source| SqlError::RuleIo {
            path: dir.to_owned(),
            source,
        };
        let mut files: Vec<_> = fs::read_dir(dir)
            .map_err(io)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            })
            .collect();
        files.sort();

        let mut set = Self::empty();
        for path in files {
            let json = fs::read_to_string(&path).map_err(|source| SqlError::RuleIo {
                path: path.clone(),
                source,
            })?;
            let file_id = path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            set.rules
                .extend(Self::from_json(&file_id, &json, &path)?.rules);
        }
        Ok(set)
    }

    /// The rules in application order.
    pub fn rules(&self) -> &[RewriteRule] {
        &self.rules
    }

    /// The number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

impl SqlRewriter for RuleSet {
    fn rewrite(&self, sql: &str) -> Rewritten {
        let mut text = sql.to_owned();
        let mut applied = Vec::new();
        for rule in &self.rules {
            if let Some(next) = rule.apply(&text) {
                text = next;
                applied.push(rule.id().to_owned());
            }
        }
        Rewritten { text, applied }
    }
}
