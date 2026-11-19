use std::fmt::Debug;

use vdp_db::DbError;
use vdp_vaultspeed_domain::{AlterOp, Evidence, ObjectType, Statement, StatementKind};

/// Interpretation of a failed statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorVerdict {
    /// The error proves the statement's effect already holds.
    Satisfied(Evidence),
    Failure,
}

/// Decides whether a database error is a real failure.
pub trait ErrorClassifier: Send + Sync + Debug {
    /// Decides whether `error` of `statement` proves its effect already holds, or is a failure.
    fn classify(&self, statement: &Statement, error: &DbError) -> ErrorVerdict;
}

/// Treats every error as a failure.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoClassification;

impl ErrorClassifier for NoClassification {
    fn classify(&self, _: &Statement, _: &DbError) -> ErrorVerdict {
        ErrorVerdict::Failure
    }
}

/// MySQL/SingleStore error codes that prove the desired state, accepted only for the
/// statement kinds where that conclusion is sound. A duplicate column (1060) on
/// `ADD COLUMN` means the column is there; the same code on anything else is a failure.
#[derive(Debug, Clone, Copy, Default)]
pub struct MySqlErrorClassifier;

impl MySqlErrorClassifier {
    const DB_CREATE_EXISTS: u16 = 1007;
    const DB_DROP_MISSING: u16 = 1008;
    const TABLE_EXISTS: u16 = 1050;
    const BAD_TABLE: u16 = 1051;
    const DUPLICATE_COLUMN: u16 = 1060;
    const CANT_DROP_FIELD: u16 = 1091;
}

impl ErrorClassifier for MySqlErrorClassifier {
    fn classify(&self, statement: &Statement, error: &DbError) -> ErrorVerdict {
        let Some(code) = error.code else {
            return ErrorVerdict::Failure;
        };
        let proves = matches!(
            (&statement.kind, code),
            (
                StatementKind::Alter {
                    op: AlterOp::AddColumn { .. }
                },
                Self::DUPLICATE_COLUMN
            ) | (
                StatementKind::Alter {
                    op: AlterOp::DropColumn { .. }
                },
                Self::CANT_DROP_FIELD
            ) | (
                StatementKind::Drop {
                    object: ObjectType::Table | ObjectType::View,
                    ..
                },
                Self::BAD_TABLE
            ) | (
                StatementKind::Drop {
                    object: ObjectType::Database,
                    ..
                },
                Self::DB_DROP_MISSING
            ) | (
                StatementKind::Create {
                    object: ObjectType::Database,
                    ..
                },
                Self::DB_CREATE_EXISTS
            ) | (
                StatementKind::Create {
                    object: ObjectType::Table,
                    or_replace: false,
                    ..
                },
                Self::TABLE_EXISTS
            )
        );
        if proves {
            ErrorVerdict::Satisfied(Evidence::ErrorCode {
                code,
                message: error.message.clone(),
            })
        } else {
            ErrorVerdict::Failure
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vdp_vaultspeed_sql::{ArtifactPipeline, NoRewrite, WholeFileSplitter};

    use super::*;

    fn statement(sql: &str) -> Statement {
        ArtifactPipeline::new(Arc::new(WholeFileSplitter), vec![], Arc::new(NoRewrite))
            .statements(sql)
            .unwrap()
            .remove(0)
    }

    fn error(code: u16) -> DbError {
        DbError {
            code: Some(code),
            message: "x".into(),
        }
    }

    #[test]
    fn duplicate_column_only_proves_add_column() {
        let add = statement("ALTER TABLE `edw_dv_rdv`.`sat` ADD COLUMN `c` DECIMAL(2)");
        assert!(matches!(
            MySqlErrorClassifier.classify(&add, &error(1060)),
            ErrorVerdict::Satisfied(_)
        ));
        let insert = statement("INSERT INTO t SELECT 1");
        assert_eq!(
            MySqlErrorClassifier.classify(&insert, &error(1060)),
            ErrorVerdict::Failure
        );
    }

    #[test]
    fn client_errors_and_unknown_codes_are_failures() {
        let add = statement("ALTER TABLE t ADD COLUMN c INT");
        assert_eq!(
            MySqlErrorClassifier.classify(&add, &DbError::client("timeout")),
            ErrorVerdict::Failure
        );
        assert_eq!(
            MySqlErrorClassifier.classify(&add, &error(1146)),
            ErrorVerdict::Failure
        );
        let replace = statement("CREATE OR REPLACE VIEW v AS SELECT 1");
        assert_eq!(
            MySqlErrorClassifier.classify(&replace, &error(1050)),
            ErrorVerdict::Failure
        );
    }
}
