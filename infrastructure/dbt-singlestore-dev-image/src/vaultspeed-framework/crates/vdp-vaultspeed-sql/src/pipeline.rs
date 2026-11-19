//! Assembling statements from a file's text.

use std::sync::Arc;

use vdp_vaultspeed_domain::Statement;

use crate::lexer::{TokenKind, tokenize};
use crate::{
    SqlError, SqlPreprocessor, SqlRewriter, StatementSplitter, classify_in, fingerprint,
    split::has_sql, use_database,
};

/// How the files of one artifact kind become statements: split, preprocess, rewrite,
/// classify, fingerprint.
#[derive(Debug, Clone)]
pub struct ArtifactPipeline {
    splitter: Arc<dyn StatementSplitter>,
    preprocessors: Vec<Arc<dyn SqlPreprocessor>>,
    rewriter: Arc<dyn SqlRewriter>,
    default_database: Option<String>,
}

impl ArtifactPipeline {
    pub fn new(
        splitter: Arc<dyn StatementSplitter>,
        preprocessors: Vec<Arc<dyn SqlPreprocessor>>,
        rewriter: Arc<dyn SqlRewriter>,
    ) -> Self {
        Self {
            splitter,
            preprocessors,
            rewriter,
            default_database: None,
        }
    }

    /// The database in effect when a script does not say (the environment's database): unqualified
    /// object names resolve to it, so they key the ledger like their qualified spelling. A `USE` in the
    /// script overrides it for the statements that follow. Empty means unknown. Fingerprints are
    /// unaffected: they hash the text, not the resolved names.
    #[must_use]
    pub fn with_default_database(mut self, database: impl Into<String>) -> Self {
        let database = database.into().trim().to_lowercase();
        self.default_database = Some(database).filter(|d| !d.is_empty());
        self
    }

    /// Statements of one file, in file order.
    ///
    /// The fingerprint of the raw (`VaultSpeed`) text drives decisions; classification
    /// uses the effective text because rewrite rules change the real target objects
    /// (`ami_inc.x` → `edw_inc.ami_x`).
    pub fn statements(&self, script: &str) -> Result<Vec<Statement>, SqlError> {
        let mut statements = Vec::new();
        let mut database = self.default_database.clone();
        for raw in self.splitter.split(script) {
            reject_client_directives(raw)?;
            let mut effective = raw.to_owned();
            for preprocessor in &self.preprocessors {
                effective = preprocessor.apply(&effective).into_owned();
            }
            let rewritten = self.rewriter.rewrite(&effective);
            let effective = rewritten.text.trim().to_owned();
            if !has_sql(&effective) {
                continue;
            }
            let classification = classify_in(&effective, database.as_deref());
            if let Some(switched) = use_database(&effective) {
                database = Some(switched);
            }
            statements.push(Statement {
                raw_fp: fingerprint(raw),
                effective_fp: fingerprint(&effective),
                raw: raw.to_owned(),
                kind: classification.kind,
                target: classification.target,
                applied_rules: rewritten.applied,
                effective,
            });
        }
        Ok(statements)
    }
}

fn reject_client_directives(raw: &str) -> Result<(), SqlError> {
    let first_word = tokenize(raw)
        .into_iter()
        .find(|t| !t.is_trivia())
        .filter(|t| t.kind == TokenKind::Word);
    match first_word {
        Some(word) if word.is_word("DELIMITER") || word.is_word("SOURCE") => {
            Err(SqlError::UnsupportedDirective {
                directive: word.text.to_owned(),
            })
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use vdp_vaultspeed_domain::{AlterOp, ObjectRef, ObjectType, StatementKind};

    use super::*;
    use crate::{NoRewrite, RoutineAwareSplitter, RuleSet, StripComments};

    fn ddl_pipeline(rules: RuleSet) -> ArtifactPipeline {
        ArtifactPipeline::new(
            Arc::new(RoutineAwareSplitter),
            vec![Arc::new(StripComments::unless_shard_key_hint())],
            Arc::new(rules),
        )
    }

    #[test]
    fn builds_classified_statements() {
        let script =
            "/* header 2025 */\nDROP TABLE IF EXISTS `s`.`t`;\nCREATE TABLE `s`.`t` (a INT);";
        let statements = ddl_pipeline(RuleSet::empty()).statements(script).unwrap();
        assert_eq!(statements.len(), 2);
        assert_eq!(
            statements[0].kind,
            StatementKind::Drop {
                object: ObjectType::Table,
                if_exists: true
            }
        );
        assert_eq!(statements[0].effective, "DROP TABLE IF EXISTS `s`.`t`");
        assert!(statements[0].raw.starts_with("/* header"));
        assert_eq!(statements[0].raw_fp, statements[0].effective_fp);
    }

    #[test]
    fn raw_fingerprint_is_stable_when_rules_change() {
        let rules = RuleSet::from_json(
            "r",
            r#"{"modifications":[{"regex_pattern":"`ami_inc`","replacement":"`edw_inc`"}]}"#,
            Path::new("r.json"),
        )
        .unwrap();
        let script = "INSERT INTO `ami_inc`.`x` SELECT 1;";
        let with = ddl_pipeline(rules).statements(script).unwrap().remove(0);
        let without = ddl_pipeline(RuleSet::empty())
            .statements(script)
            .unwrap()
            .remove(0);
        assert_eq!(with.raw_fp, without.raw_fp);
        assert_ne!(with.effective_fp, without.effective_fp);
        assert_eq!(with.applied_rules, vec!["r#0"]);
        assert_eq!(with.target.unwrap().qualified_name(), "edw_inc.x");
    }

    #[test]
    fn unqualified_targets_follow_the_default_database_and_use() {
        let pipeline =
            ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), vec![], Arc::new(NoRewrite))
                .with_default_database("EDW");
        let script = "ALTER TABLE x RENAME TO x_old;\nUSE other;\nCREATE TABLE y (a INT);";
        let statements = pipeline.statements(script).unwrap();
        let name = |i: usize| statements[i].target.as_ref().map(ObjectRef::qualified_name);
        assert_eq!(name(0).as_deref(), Some("edw.x"));
        assert_eq!(name(2).as_deref(), Some("other.y"));
        let StatementKind::Alter {
            op: AlterOp::Rename { to },
        } = &statements[0].kind
        else {
            panic!("expected rename");
        };
        assert_eq!(to.qualified_name(), "edw.x_old");

        // The text, and so the fingerprint, is the same without a default database.
        let plain =
            ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), vec![], Arc::new(NoRewrite))
                .statements(script)
                .unwrap();
        assert_eq!(plain[0].raw_fp, statements[0].raw_fp);
        assert_eq!(plain[0].target.as_ref().unwrap().qualified_name(), "x");
    }

    #[test]
    fn rejects_delimiter_directives() {
        let pipeline =
            ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), vec![], Arc::new(NoRewrite));
        assert!(matches!(
            pipeline.statements("DELIMITER //\nSELECT 1;"),
            Err(SqlError::UnsupportedDirective { .. })
        ));
    }
}
