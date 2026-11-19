use std::fmt;

use regex::{Captures, Regex};
use serde::Deserialize;

use super::python;
use crate::SqlError;

/// One rule as written in `QUERY_MODIFICATIONS.json`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RuleSpec {
    #[serde(default)]
    pub(crate) reason: String,
    pub(crate) regex_pattern: String,
    #[serde(default)]
    pub(crate) replacement: String,
    #[serde(default)]
    pub(crate) exclusions: Vec<String>,
    /// Capture groups the exclusions are tested against; the whole match when absent.
    #[serde(default)]
    pub(crate) exclusion_groups: Option<Vec<usize>>,
}

/// A compiled regex substitution with per-match exclusions.
///
/// Every match is considered on its own: a match is left untouched when any exclusion
/// regex matches the inspected text (the whole match, or the configured capture
/// groups). The Python implementation only inspected group 2 of the *first* match and
/// then skipped the rule for the entire statement.
#[derive(Clone)]
pub struct RewriteRule {
    id: String,
    reason: String,
    pattern: Regex,
    replacement: String,
    exclusions: Vec<Regex>,
    exclusion_groups: Option<Vec<usize>>,
}

impl RewriteRule {
    pub(crate) fn compile(id: String, spec: &RuleSpec) -> Result<Self, SqlError> {
        let compile = |p: &str| {
            Regex::new(&python::pattern(p)).map_err(|source| SqlError::InvalidRulePattern {
                rule: id.clone(),
                source: Box::new(source),
            })
        };
        Ok(Self {
            pattern: compile(&spec.regex_pattern)?,
            exclusions: spec
                .exclusions
                .iter()
                .map(|e| compile(e))
                .collect::<Result<_, _>>()?,
            replacement: python::replacement(&spec.replacement),
            reason: spec.reason.clone(),
            exclusion_groups: spec.exclusion_groups.clone(),
            id,
        })
    }

    /// The rule id, `<file>#<index>`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Why the rule exists.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the rewritten text when the rule changed anything.
    pub fn apply(&self, sql: &str) -> Option<String> {
        if !self.pattern.is_match(sql) {
            return None;
        }
        let result = self.pattern.replace_all(sql, |caps: &Captures<'_>| {
            if self.is_excluded(caps) {
                caps[0].to_owned()
            } else {
                let mut expanded = String::new();
                caps.expand(&self.replacement, &mut expanded);
                expanded
            }
        });
        (result != sql).then(|| result.into_owned())
    }

    fn is_excluded(&self, caps: &Captures<'_>) -> bool {
        if self.exclusions.is_empty() {
            return false;
        }
        let inspected: Vec<&str> = match &self.exclusion_groups {
            Some(groups) => groups
                .iter()
                .filter_map(|g| caps.get(*g).map(|m| m.as_str()))
                .collect(),
            None => vec![caps.get(0).map_or("", |m| m.as_str())],
        };
        inspected
            .iter()
            .any(|text| self.exclusions.iter().any(|ex| ex.is_match(text)))
    }
}

impl fmt::Debug for RewriteRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RewriteRule")
            .field("id", &self.id)
            .field("reason", &self.reason)
            .field("pattern", &self.pattern.as_str())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, replacement: &str, exclusions: &[&str]) -> RewriteRule {
        RewriteRule::compile(
            "test#0".into(),
            &RuleSpec {
                reason: String::new(),
                regex_pattern: pattern.into(),
                replacement: replacement.into(),
                exclusions: exclusions.iter().map(|s| (*s).to_owned()).collect(),
                exclusion_groups: None,
            },
        )
        .unwrap()
    }

    const SCHEMA_RULE: &str = r"(EXISTS|TABLE|FROM|JOIN|INTO|VIEW|TRUNCATE|DROP|UPDATE|MERGE|REPLACE)\s`?([a-z0-9_]+)_(inc|stg|ext|ini|dfv|mtd)`?\.`([a-z0-9_]+)`";
    const SCHEMA_REPLACEMENT: &str = r"\1 `edw_\3`.`\2_\4`";
    const SCHEMA_EXCLUSION: &str =
        "(edw_fmc|edw_ext|edw_stg|edw_mtd|edw_rdv|edw_bv|edw_dfv|edw_inc|edw_ini)";

    #[test]
    fn rewrites_source_schemas_into_shared_schemas() {
        let r = rule(SCHEMA_RULE, SCHEMA_REPLACEMENT, &[SCHEMA_EXCLUSION]);
        assert_eq!(
            r.apply("SELECT * FROM `ami_inc`.`persoon` JOIN ami_stg.`x`")
                .unwrap(),
            "SELECT * FROM `edw_inc`.`ami_persoon` JOIN `edw_stg`.`ami_x`"
        );
    }

    #[test]
    fn exclusions_apply_per_match() {
        let r = rule(SCHEMA_RULE, SCHEMA_REPLACEMENT, &[SCHEMA_EXCLUSION]);
        assert_eq!(
            r.apply("FROM `edw_inc`.`ami_x` JOIN `ami_inc`.`y`")
                .unwrap(),
            "FROM `edw_inc`.`ami_x` JOIN `edw_inc`.`ami_y`",
            "already-rewritten names must stay, others must still be rewritten"
        );
        assert!(r.apply("FROM `edw_inc`.`ami_x`").is_none());
    }

    #[test]
    fn multiline_and_backreferences() {
        let r = rule(
            r"(CREATE {1,15}TABLE {1,8}`\w+`.`\w+`[^;]+)^\)\s([^;]+)",
            r"\1\2)\n",
            &[],
        );
        let sql = "CREATE TABLE `a`.`b`\n(\n x INT\n)\n-- , SHARD KEY (`x`)\n";
        assert_eq!(
            r.apply(sql).unwrap(),
            "CREATE TABLE `a`.`b`\n(\n x INT\n-- , SHARD KEY (`x`)\n)\n"
        );
    }
}
