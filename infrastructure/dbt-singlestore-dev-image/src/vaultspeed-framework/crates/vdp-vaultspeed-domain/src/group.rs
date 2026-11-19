use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{ArtifactKind, Fingerprint, JobName, Layer, ObjectRef, OrderKey, RelPath, Statement};

/// Stable identifier of a statement group within one source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(String);

impl GroupId {
    /// The id of the group at position `ordinal` of the file `rel_path` of `source`, as `source:rel_path#ordinal`.
    pub fn new(source: &str, rel_path: &RelPath, ordinal: usize) -> Self {
        Self(format!("{source}:{rel_path}#{ordinal}"))
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Everything derived from the file a group came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    pub artifact: ArtifactKind,
    pub job: JobName,
    /// Path of the source file relative to the artifact root of its kind
    /// (for a COMP zip: relative to `DDL/` or `ETL/`).
    pub rel_path: RelPath,
    pub layer: Layer,
    pub order: OrderKey,
}

/// Statements that are decided, executed and recorded as one unit, e.g. the
/// `DROP TABLE IF EXISTS x` + `CREATE TABLE x` pair `VaultSpeed` emits for every table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementGroup {
    pub id: GroupId,
    pub info: JobInfo,
    /// Position of the group within its file; keeps ordering stable.
    pub ordinal: usize,
    pub statements: Vec<Statement>,
}

impl StatementGroup {
    /// The statement that determines the group's nature: its last CREATE, else its
    /// first statement.
    pub fn head(&self) -> &Statement {
        self.statements
            .iter()
            .rev()
            .find(|s| s.kind.is_create())
            .or_else(|| self.statements.first())
            .expect("a statement group is never empty")
    }

    /// The ordering rank of the group's head statement (see [`crate::StatementKind::rank`]).
    pub fn rank(&self) -> u8 {
        self.head().kind.rank()
    }

    /// The raw fingerprints of all statements, combined in order.
    pub fn raw_fp(&self) -> Fingerprint {
        Fingerprint::combine(self.statements.iter().map(|s| &s.raw_fp))
    }

    /// The objects the statements target, in statement order.
    pub fn targets(&self) -> impl Iterator<Item = &ObjectRef> {
        self.statements.iter().filter_map(|s| s.target.as_ref())
    }

    /// Sort key inside a phase: order number, job, statement rank, file position.
    pub fn sort_key(&self) -> (&OrderKey, &JobName, u8, &RelPath, usize) {
        (
            &self.info.order,
            &self.info.job,
            self.rank(),
            &self.info.rel_path,
            self.ordinal,
        )
    }
}
