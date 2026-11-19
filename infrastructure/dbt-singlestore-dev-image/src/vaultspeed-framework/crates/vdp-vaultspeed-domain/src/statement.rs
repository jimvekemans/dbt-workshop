use serde::{Deserialize, Serialize};

use crate::{Facet, Fingerprint, ObjectKey, ObjectRef, ObjectType};

/// What a single SQL statement does, as far as idempotency is concerned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatementKind {
    Create {
        object: ObjectType,
        or_replace: bool,
        if_not_exists: bool,
    },
    Drop {
        object: ObjectType,
        if_exists: bool,
    },
    Alter {
        op: AlterOp,
    },
    Truncate,
    Insert,
    Update,
    Delete,
    Merge,
    Replace,
    Call,
    Set,
    Select,
    Use,
    Other,
}

/// The single operation performed by an `ALTER TABLE`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AlterOp {
    AddColumn {
        column: String,
        data_type: String,
    },
    ModifyColumn {
        column: String,
        data_type: String,
    },
    ChangeColumn {
        column: String,
    },
    DropColumn {
        column: String,
    },
    Rename {
        to: ObjectRef,
    },
    Comment,
    /// Several comma-separated operations in one statement.
    Multiple,
    Other,
}

impl StatementKind {
    /// Rank used to order statements of one job: DROP < CREATE < ALTER < INSERT < rest.
    /// Matches the `QUERY_TYPE_CODE` ordering of the Python implementation.
    pub const fn rank(&self) -> u8 {
        match self {
            Self::Drop { .. } => 0,
            Self::Create { .. } => 10,
            Self::Alter { .. } => 20,
            Self::Insert => 30,
            _ => 100,
        }
    }

    /// Whether the statement is a DROP.
    pub const fn is_drop(&self) -> bool {
        matches!(self, Self::Drop { .. })
    }

    /// Whether the statement is a CREATE.
    pub const fn is_create(&self) -> bool {
        matches!(self, Self::Create { .. })
    }

    /// Human readable label, e.g. `CREATE OR REPLACE PROCEDURE` or `ALTER TABLE ADD COLUMN`.
    pub fn label(&self) -> String {
        match self {
            Self::Create {
                object, or_replace, ..
            } => format!(
                "CREATE {}{}",
                if *or_replace { "OR REPLACE " } else { "" },
                object.as_str().to_uppercase()
            ),
            Self::Drop { object, .. } => format!("DROP {}", object.as_str().to_uppercase()),
            Self::Alter { op } => {
                let suffix = match op {
                    AlterOp::AddColumn { .. } => " ADD COLUMN",
                    AlterOp::ModifyColumn { .. } => " MODIFY COLUMN",
                    AlterOp::ChangeColumn { .. } => " CHANGE COLUMN",
                    AlterOp::DropColumn { .. } => " DROP COLUMN",
                    AlterOp::Rename { .. } => " RENAME",
                    AlterOp::Comment => " COMMENT",
                    AlterOp::Multiple => " (MULTIPLE)",
                    AlterOp::Other => "",
                };
                format!("ALTER TABLE{suffix}")
            }
            Self::Truncate => "TRUNCATE".into(),
            Self::Insert => "INSERT".into(),
            Self::Update => "UPDATE".into(),
            Self::Delete => "DELETE".into(),
            Self::Merge => "MERGE".into(),
            Self::Replace => "REPLACE".into(),
            Self::Call => "CALL".into(),
            Self::Set => "SET".into(),
            Self::Select => "SELECT".into(),
            Self::Use => "USE".into(),
            Self::Other => "OTHER".into(),
        }
    }
}

/// One executable SQL statement with its provenance and identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// Text exactly as `VaultSpeed` (or the PML author) wrote it.
    pub raw: String,
    /// Text after preprocessing and rewrite rules; this is what gets executed.
    pub effective: String,
    pub kind: StatementKind,
    pub target: Option<ObjectRef>,
    /// Identity used for decisions: stable when rewrite rules change.
    pub raw_fp: Fingerprint,
    /// Identity of what actually ran; used for audit and for legacy state that only
    /// recorded executed SQL.
    pub effective_fp: Fingerprint,
    /// Ids of the rewrite rules that changed this statement.
    pub applied_rules: Vec<String>,
}

impl Statement {
    /// The ledger key of the state this statement changes (see [`Facet`]). Statements
    /// that change no tracked state (`CALL`, `SET`, `SELECT`, ...) have none.
    pub fn object_key(&self) -> Option<ObjectKey> {
        let object = self.target.clone()?;
        let facet = match &self.kind {
            StatementKind::Create { .. } | StatementKind::Drop { .. } => Facet::Definition,
            StatementKind::Alter { op } => match op {
                AlterOp::Comment => Facet::Comment,
                AlterOp::AddColumn { column, .. }
                | AlterOp::ModifyColumn { column, .. }
                | AlterOp::ChangeColumn { column }
                | AlterOp::DropColumn { column } => Facet::Column(column.clone()),
                AlterOp::Rename { .. } => Facet::Definition,
                AlterOp::Multiple | AlterOp::Other => Facet::Alteration,
            },
            StatementKind::Insert
            | StatementKind::Update
            | StatementKind::Delete
            | StatementKind::Merge
            | StatementKind::Replace
            | StatementKind::Truncate => Facet::Data,
            StatementKind::Call
            | StatementKind::Set
            | StatementKind::Select
            | StatementKind::Use
            | StatementKind::Other => return None,
        };
        Some(ObjectKey { object, facet })
    }

    /// The definition key of the object this statement targets. Everything recorded
    /// about an object (columns, rows, comment) is void once its definition is replaced.
    pub fn definition_key(&self) -> Option<ObjectKey> {
        self.target.clone().map(ObjectKey::definition)
    }
}
