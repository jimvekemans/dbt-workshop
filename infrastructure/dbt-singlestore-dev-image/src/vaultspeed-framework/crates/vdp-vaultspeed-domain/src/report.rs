use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ArtifactKind, Decision, Environment, Evidence, Fingerprint, RunId, RunMode};

/// What happened to one statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum StatementOutcome {
    Applied,
    Satisfied {
        evidence: Evidence,
    },
    Failed {
        code: Option<u16>,
        message: String,
    },
    /// The group was skipped, or an earlier statement of the group failed.
    NotRun,
}

impl StatementOutcome {
    /// Whether the statement failed.
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// The result of one statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementReport {
    pub kind: String,
    pub target: Option<String>,
    pub raw_fp: Fingerprint,
    pub effective_sql: String,
    pub outcome: StatementOutcome,
}

/// The result of one statement group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupReport {
    pub group_id: String,
    pub artifact: ArtifactKind,
    pub job: String,
    pub rel_path: String,
    pub phase: String,
    pub decision: Decision,
    pub statements: Vec<StatementReport>,
}

impl GroupReport {
    /// Whether any statement failed.
    pub fn failed(&self) -> bool {
        self.statements.iter().any(|s| s.outcome.is_failure())
    }
}

/// The result of publishing one DAG.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagReport {
    pub source_file: String,
    pub target_file: String,
    pub transformations: Vec<String>,
    pub published: bool,
    /// Unified diff between the `VaultSpeed` original and the published DAG.
    pub diff: Option<String>,
    pub error: Option<String>,
}

/// What happened to one source (PML or a generation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReport {
    pub source: String,
    pub groups: Vec<GroupReport>,
    pub dags: Vec<DagReport>,
    /// Set when a failure policy stopped processing.
    pub halted: Option<String>,
    /// Errors that are not tied to a statement (unreadable zip, publish failure, ...).
    pub errors: Vec<String>,
    /// Human readable notes about staging/archive housekeeping.
    pub housekeeping: Vec<String>,
}

impl SourceReport {
    /// Creates an empty report for `source`.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            groups: Vec::new(),
            dags: Vec::new(),
            halted: None,
            errors: Vec::new(),
            housekeeping: Vec::new(),
        }
    }

    /// Whether the source has an error, a failed statement or a DAG that failed.
    pub fn has_failures(&self) -> bool {
        !self.errors.is_empty()
            || self.groups.iter().any(GroupReport::failed)
            || self.dags.iter().any(|d| d.error.is_some())
    }
}

/// Counts over a whole run: statements applied, satisfied and failed, groups executed and skipped, DAGs published.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTotals {
    pub applied: usize,
    pub satisfied: usize,
    pub failed: usize,
    pub skipped_groups: usize,
    pub executed_groups: usize,
    pub dags: usize,
}

/// The result of one run over every source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: RunId,
    pub environment: Environment,
    pub mode: RunMode,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub sources: Vec<SourceReport>,
}

impl RunReport {
    /// Whether any source has failures.
    pub fn has_failures(&self) -> bool {
        self.sources.iter().any(SourceReport::has_failures)
    }

    /// Adds up the statement outcomes, group decisions and published DAGs of every source.
    pub fn totals(&self) -> RunTotals {
        let mut totals = RunTotals::default();
        for source in &self.sources {
            totals.dags += source.dags.iter().filter(|d| d.published).count();
            for group in &source.groups {
                if group.decision.is_execute() {
                    totals.executed_groups += 1;
                } else {
                    totals.skipped_groups += 1;
                }
                for statement in &group.statements {
                    match statement.outcome {
                        StatementOutcome::Applied => totals.applied += 1,
                        StatementOutcome::Satisfied { .. } => totals.satisfied += 1,
                        StatementOutcome::Failed { .. } => totals.failed += 1,
                        StatementOutcome::NotRun => {}
                    }
                }
            }
        }
        totals
    }
}
