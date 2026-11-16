//! Assembling statements from a file's text.

use std::sync::Arc;

use vdp_vaultspeed_domain::Statement;

use crate::lexer::{TokenKind, tokenize};
use crate::{
    SqlError, SqlPreprocessor, SqlRewriter, StatementSplitter, classify, fingerprint,
    split::has_sql,
};

/// How the files of one artifact kind become statements: split, preprocess, rewrite,
/// classify, fingerprint.
#[derive(Debug, Clone)]
pub struct ArtifactPipeline {
    splitter: Arc<dyn StatementSplitter>,
    preprocessors: Vec<Arc<dyn SqlPreprocessor>>,
    rewriter: Arc<dyn SqlRewriter>,
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
        }
    }

    /// Statements of one file, in file order.
    ///
    /// The fingerprint of the raw (`VaultSpeed`) text drives decisions; classification
    /// uses the effective text because rewrite rules change the real target objects
    /// (`ami_inc.x` → `edw_inc.ami_x`).
    pub fn statements(&self, script: &str) -> Result<Vec<Statement>, SqlError> {
        let mut statements = Vec::new();
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
            let classification = classify(&effective);
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

    use vdp_vaultspeed_domain::{ObjectType, StatementKind};

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
    fn rejects_delimiter_directives() {
        let pipeline =
            ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), vec![], Arc::new(NoRewrite));
        assert!(matches!(
            pipeline.statements("DELIMITER //\nSELECT 1;"),
            Err(SqlError::UnsupportedDirective { .. })
        ));
    }
}
