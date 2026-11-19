use std::sync::Arc;

use crate::planner::{CatalogProbe, ProbeError, ProbeVerdict};
use vdp_db::{SqlSession, SqlValue};
use vdp_vaultspeed_domain::{AlterOp, Evidence, ObjectRef, ObjectType, Statement, StatementKind};

/// [`CatalogProbe`] backed by `information_schema`.
#[derive(Debug, Clone)]
pub struct InformationSchemaProbe {
    session: Arc<dyn SqlSession>,
}

impl InformationSchemaProbe {
    /// Creates a probe that asks `information_schema` through `session`.
    pub fn new(session: Arc<dyn SqlSession>) -> Self {
        Self { session }
    }

    fn table_exists(&self, schema: &str, table: &str) -> Result<bool, ProbeError> {
        let rows = self
            .session
            .query(
                "SELECT 1 FROM information_schema.TABLES \
                 WHERE LOWER(TABLE_SCHEMA) = ? AND LOWER(TABLE_NAME) = ?",
                &[schema.into(), table.into()],
            )
            .map_err(|e| ProbeError(vdp_core::display_chain(&e)))?;
        Ok(!rows.is_empty())
    }

    fn column_type(
        &self,
        schema: &str,
        table: &str,
        column: &str,
    ) -> Result<Option<String>, ProbeError> {
        let rows = self
            .session
            .query(
                "SELECT COLUMN_TYPE FROM information_schema.COLUMNS \
                 WHERE LOWER(TABLE_SCHEMA) = ? AND LOWER(TABLE_NAME) = ? AND LOWER(COLUMN_NAME) = ?",
                &[schema.into(), table.into(), column.into()],
            )
            .map_err(|e| ProbeError(vdp_core::display_chain(&e)))?;
        Ok(rows.first().map(|row| {
            row.first()
                .and_then(SqlValue::as_text)
                .unwrap_or_default()
                .to_owned()
        }))
    }
}

impl CatalogProbe for InformationSchemaProbe {
    fn exists(&self, object: &ObjectRef) -> Result<Option<bool>, ProbeError> {
        match (&object.schema, object.object_type) {
            (Some(schema), ObjectType::Table | ObjectType::View) => {
                self.table_exists(schema, &object.name).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn verify(&self, statement: &Statement) -> Result<ProbeVerdict, ProbeError> {
        let Some(ObjectRef {
            schema: Some(schema),
            name,
            ..
        }) = &statement.target
        else {
            return Ok(ProbeVerdict::Unknown);
        };
        let table = format!("{schema}.{name}");
        let verdict = match &statement.kind {
            StatementKind::Create {
                object: ObjectType::Table,
                or_replace: false,
                ..
            } if self.table_exists(schema, name)? => {
                ProbeVerdict::Holds(Evidence::TablePresent { table })
            }
            StatementKind::Alter {
                op: AlterOp::AddColumn { column, data_type },
            } => match self.column_type(schema, name, column)? {
                None => ProbeVerdict::Unknown,
                Some(actual) if types_compatible(data_type, &actual) => {
                    ProbeVerdict::Holds(Evidence::ColumnPresent {
                        table,
                        column: column.clone(),
                        data_type: actual,
                    })
                }
                Some(actual) => ProbeVerdict::Contradicts(format!(
                    "column {table}.{column} already exists as `{actual}`, \
                     but this generation adds it as `{data_type}`"
                )),
            },
            StatementKind::Alter {
                op: AlterOp::ModifyColumn { column, data_type },
            } => match self.column_type(schema, name, column)? {
                Some(actual) if types_compatible(data_type, &actual) => {
                    ProbeVerdict::Holds(Evidence::ColumnPresent {
                        table,
                        column: column.clone(),
                        data_type: actual,
                    })
                }
                _ => ProbeVerdict::Unknown,
            },
            StatementKind::Alter {
                op: AlterOp::DropColumn { column },
            } if self.table_exists(schema, name)?
                && self.column_type(schema, name, column)?.is_none() =>
            {
                ProbeVerdict::Holds(Evidence::ColumnAbsent {
                    table,
                    column: column.clone(),
                })
            }
            StatementKind::Alter {
                op: AlterOp::Rename { to },
            } => {
                let to_schema = to.schema.as_deref().unwrap_or(schema);
                match (
                    self.table_exists(schema, name)?,
                    self.table_exists(to_schema, &to.name)?,
                ) {
                    (false, true) => ProbeVerdict::Holds(Evidence::AlreadyRenamed {
                        from: table,
                        to: to.qualified_name(),
                    }),
                    (true, true) => ProbeVerdict::Contradicts(format!(
                        "both {table} and the rename target {} exist",
                        to.qualified_name()
                    )),
                    _ => ProbeVerdict::Unknown,
                }
            }
            _ => ProbeVerdict::Unknown,
        };
        Ok(verdict)
    }
}

/// Whether a declared column type (`DECIMAL(2)`, `INTEGER`) denotes the type the
/// catalog reports (`decimal(2,0)`, `int(11)`).
///
/// Aliases are resolved, integer display widths are ignored, `DECIMAL(p)` means
/// `decimal(p,0)` and bare `DECIMAL` means `decimal(10,0)`. A declared type without
/// parameters accepts the catalog's defaults for that base type.
pub(crate) fn types_compatible(declared: &str, actual: &str) -> bool {
    let declared = ColumnType::parse(declared);
    let actual = ColumnType::parse(actual);
    if declared.base != actual.base || declared.unsigned != actual.unsigned {
        return false;
    }
    if INTEGER_TYPES.contains(&declared.base.as_str()) {
        return true;
    }
    match (&declared.params, &actual.params) {
        (None, _) => true,
        (Some(d), Some(a)) => d == a,
        (Some(_), None) => false,
    }
}

const INTEGER_TYPES: [&str; 5] = ["tinyint", "smallint", "mediumint", "int", "bigint"];

#[derive(Debug, PartialEq, Eq)]
struct ColumnType {
    base: String,
    params: Option<String>,
    unsigned: bool,
}

impl ColumnType {
    fn parse(text: &str) -> Self {
        let text = text.to_ascii_lowercase();
        let compact: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let (head, params, tail) = match (compact.find('('), compact.rfind(')')) {
            (Some(open), Some(close)) if open < close => (
                compact[..open].trim().to_owned(),
                Some(compact[open + 1..close].replace(' ', "")),
                compact[close + 1..].to_owned(),
            ),
            _ => (compact.clone(), None, String::new()),
        };
        let unsigned = tail.contains("unsigned") || head.contains("unsigned");
        let head = head.replace("unsigned", "").trim().to_owned();
        let (base, params) = match head.as_str() {
            "integer" => ("int".to_owned(), params),
            "bool" | "boolean" => ("tinyint".to_owned(), Some("1".to_owned())),
            "numeric" | "dec" | "fixed" | "decimal" => (
                "decimal".to_owned(),
                Some(match params.as_deref() {
                    None => "10,0".to_owned(),
                    Some(p) if !p.contains(',') => format!("{p},0"),
                    Some(p) => p.to_owned(),
                }),
            ),
            "double precision" | "real" => ("double".to_owned(), params),
            "character" => ("char".to_owned(), params),
            "character varying" => ("varchar".to_owned(), params),
            other => (other.to_owned(), params),
        };
        Self {
            base,
            params,
            unsigned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::types_compatible;

    #[test]
    fn compatible_types() {
        for (declared, actual) in [
            ("varchar(6)", "varchar(6)"),
            ("VARCHAR ( 6 )", "varchar(6)"),
            ("decimal", "decimal(10,0)"),
            ("decimal(2)", "decimal(2,0)"),
            ("integer", "int(11)"),
            ("bigint", "bigint(20)"),
            ("datetime(6)", "datetime(6)"),
            ("datetime", "datetime"),
            ("int unsigned", "int(10) unsigned"),
        ] {
            assert!(types_compatible(declared, actual), "{declared} ~ {actual}");
        }
    }

    #[test]
    fn incompatible_types() {
        for (declared, actual) in [
            ("varchar(6)", "varchar(90)"),
            ("decimal(2)", "decimal(10,0)"),
            ("int", "bigint(20)"),
            ("int", "int(10) unsigned"),
            ("datetime(6)", "datetime"),
            ("varchar(6)", "text"),
        ] {
            assert!(
                !types_compatible(declared, actual),
                "{declared} !~ {actual}"
            );
        }
    }
}
