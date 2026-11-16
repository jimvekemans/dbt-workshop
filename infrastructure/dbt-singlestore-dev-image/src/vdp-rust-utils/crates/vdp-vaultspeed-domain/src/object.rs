use std::fmt;

use serde::{Deserialize, Serialize};

/// The kind of database object a statement touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectType {
    Table,
    View,
    Procedure,
    Function,
    Database,
    Index,
    Other,
}

impl ObjectType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::View => "view",
            Self::Procedure => "procedure",
            Self::Function => "function",
            Self::Database => "database",
            Self::Index => "index",
            Self::Other => "object",
        }
    }

    /// Objects that can be redefined wholesale without losing data.
    pub const fn is_replaceable(self) -> bool {
        matches!(self, Self::View | Self::Procedure | Self::Function)
    }
}

/// A (possibly schema-qualified) database object; names are stored without backticks
/// and lower-cased, since `SingleStore` resolves them case-insensitively in practice.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ObjectRef {
    pub object_type: ObjectType,
    pub schema: Option<String>,
    pub name: String,
}

impl ObjectRef {
    pub fn new(object_type: ObjectType, schema: Option<&str>, name: &str) -> Self {
        Self {
            object_type,
            schema: schema.map(clean_identifier).filter(|s| !s.is_empty()),
            name: clean_identifier(name),
        }
    }

    pub fn qualified_name(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.name),
            None => self.name.clone(),
        }
    }

    /// Same schema and name, ignoring the object type (`DROP TABLE x` / `CREATE VIEW x`).
    pub fn same_name(&self, other: &Self) -> bool {
        self.schema == other.schema && self.name == other.name
    }

    #[must_use]
    pub fn with_type(&self, object_type: ObjectType) -> Self {
        Self {
            object_type,
            ..self.clone()
        }
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.object_type.as_str(), self.qualified_name())
    }
}

/// Which aspect of an object a statement changes. Each facet has its own ledger history,
/// so an `ADD COLUMN` never looks like a new version of the table definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Facet {
    /// Existence and shape of the object: CREATE, DROP, RENAME.
    Definition,
    /// The table comment (`ALTER TABLE ... COMMENT`).
    Comment,
    /// One column (`ADD`/`MODIFY`/`CHANGE`/`DROP COLUMN`).
    Column(String),
    /// Rows (INSERT, UPDATE, DELETE, MERGE, REPLACE, TRUNCATE).
    Data,
    /// Any other `ALTER TABLE` (indexes, several operations at once, ...).
    Alteration,
}

/// Ledger key for "latest version of this object" lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectKey {
    pub object: ObjectRef,
    pub facet: Facet,
}

impl ObjectKey {
    pub const fn definition(object: ObjectRef) -> Self {
        Self {
            object,
            facet: Facet::Definition,
        }
    }

    pub const fn comment(object: ObjectRef) -> Self {
        Self {
            object,
            facet: Facet::Comment,
        }
    }

    pub fn is_definition(&self) -> bool {
        self.facet == Facet::Definition
    }
}

impl fmt::Display for ObjectKey {
    /// Stable textual form stored in ledgers, e.g. `view:edw_dv_bdv.sat_x` or
    /// `table:edw_dv_rdv.sat_x#comment`. Keys are type-agnostic for tables/views because
    /// `VaultSpeed` sometimes swaps one for the other under the same name.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let family = match self.object.object_type {
            ObjectType::Table | ObjectType::View => "relation",
            other => other.as_str(),
        };
        write!(f, "{family}:{}", self.object.qualified_name())?;
        match &self.facet {
            Facet::Definition => Ok(()),
            Facet::Comment => f.write_str("#comment"),
            Facet::Column(column) => write!(f, "#column:{column}"),
            Facet::Data => f.write_str("#data"),
            Facet::Alteration => f.write_str("#alter"),
        }
    }
}

fn clean_identifier(raw: &str) -> String {
    raw.trim()
        .trim_matches('`')
        .trim_matches('"')
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_normalised() {
        let obj = ObjectRef::new(ObjectType::Table, Some("`EDW_DV_RDV`"), "`Sat_X`");
        assert_eq!(obj.qualified_name(), "edw_dv_rdv.sat_x");
    }

    #[test]
    fn object_keys_are_stable_text() {
        let table = ObjectRef::new(ObjectType::Table, Some("s"), "t");
        assert_eq!(
            ObjectKey::definition(table.clone()).to_string(),
            "relation:s.t"
        );
        assert_eq!(
            ObjectKey::comment(table.clone()).to_string(),
            "relation:s.t#comment"
        );
        let column = ObjectKey {
            object: table,
            facet: Facet::Column("c".into()),
        };
        assert_eq!(column.to_string(), "relation:s.t#column:c");
        let proc_ = ObjectRef::new(ObjectType::Procedure, Some("edw_proc"), "p");
        assert_eq!(
            ObjectKey::definition(proc_).to_string(),
            "procedure:edw_proc.p"
        );
    }
}
