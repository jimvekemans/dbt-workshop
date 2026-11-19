//! Batch work as typed records: one [`BatchRecord`] per item, each ending as succeeded, failed or skipped.
//!
//! A command that works through many items (tickets, databases, files) and carries on past a failing item
//! collects what happened to every item in a [`BatchReport`], so partial failures are auditable and always map
//! to the same [`Outcome`]: any failure is exit 1, everything else exit 0. Failures that stop the whole batch
//! (authentication, setup) are not records: they stay `Err` and end the command with exit 2.

use serde::{Deserialize, Serialize};

use crate::Outcome;

/// How one item of a batch ended.
///
/// `S` is what a success reports, `F` what a failure reports. A skipped item was deliberately left alone and
/// only carries the reason; it is neither a success nor a failure.
///
/// ```
/// use vdp_core::BatchOutcome;
///
/// let done: BatchOutcome<u32, String> = BatchOutcome::Succeeded(3);
/// let left_alone: BatchOutcome<u32, String> = BatchOutcome::Skipped { reason: "up to date".into() };
/// assert!(done.is_succeeded());
/// assert!(!left_alone.is_failed());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchOutcome<S, F> {
    /// The item was processed.
    Succeeded(S),
    /// The item could not be processed; the batch carried on.
    Failed(F),
    /// The item was deliberately left alone.
    Skipped {
        /// Why the item was left alone.
        reason: String,
    },
}

impl<S, F> BatchOutcome<S, F> {
    /// Whether the item succeeded.
    pub const fn is_succeeded(&self) -> bool {
        matches!(self, Self::Succeeded(_))
    }

    /// Whether the item failed.
    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    /// Whether the item was skipped.
    pub const fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }
}

/// One item of a batch and how it ended.
///
/// ```
/// use vdp_core::{BatchOutcome, BatchRecord};
///
/// let record: BatchRecord<&str, (), String> = BatchRecord::new("INTEL-1", BatchOutcome::Succeeded(()));
/// assert_eq!(record.key, "INTEL-1");
/// assert!(record.outcome.is_succeeded());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRecord<K, S, F> {
    /// What identifies the item (a ticket key, a database name, a file path, ...).
    pub key: K,
    /// How the item ended.
    pub outcome: BatchOutcome<S, F>,
}

impl<K, S, F> BatchRecord<K, S, F> {
    /// A record for `key` with its `outcome`.
    pub const fn new(key: K, outcome: BatchOutcome<S, F>) -> Self {
        Self { key, outcome }
    }
}

/// How many items of a [`BatchReport`] ended each way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BatchCounts {
    /// Items that succeeded.
    pub succeeded: usize,
    /// Items that failed.
    pub failed: usize,
    /// Items that were skipped.
    pub skipped: usize,
}

impl BatchCounts {
    /// All items, however they ended.
    pub const fn total(self) -> usize {
        self.succeeded + self.failed + self.skipped
    }
}

/// What happened to every item of a batch, in the order the items were handled.
///
/// ```
/// use vdp_core::{BatchReport, Outcome};
///
/// let mut report: BatchReport<&str, u32, String> = BatchReport::new();
/// report.push_succeeded("a.sql", 12);
/// report.push_failed("b.sql", "syntax error".to_owned());
/// report.push_skipped("c.sql", "not selected");
///
/// let counts = report.counts();
/// assert_eq!((counts.succeeded, counts.failed, counts.skipped), (1, 1, 1));
/// assert!(report.has_failures());
/// assert_eq!(report.outcome(), Outcome::Failures);
/// assert_eq!(report.failed().map(|(key, _)| *key).collect::<Vec<_>>(), ["b.sql"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchReport<K, S, F> {
    records: Vec<BatchRecord<K, S, F>>,
}

impl<K, S, F> Default for BatchReport<K, S, F> {
    fn default() -> Self {
        Self {
            records: Vec::new(),
        }
    }
}

impl<K, S, F> BatchReport<K, S, F> {
    /// An empty report.
    pub const fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// An empty report with room for `capacity` records.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            records: Vec::with_capacity(capacity),
        }
    }

    /// Appends the `outcome` of the item `key`.
    pub fn push(&mut self, key: K, outcome: BatchOutcome<S, F>) {
        self.push_record(BatchRecord::new(key, outcome));
    }

    /// Appends an already built record.
    pub fn push_record(&mut self, record: BatchRecord<K, S, F>) {
        self.records.push(record);
    }

    /// Appends a success.
    pub fn push_succeeded(&mut self, key: K, success: S) {
        self.push(key, BatchOutcome::Succeeded(success));
    }

    /// Appends a failure.
    pub fn push_failed(&mut self, key: K, failure: F) {
        self.push(key, BatchOutcome::Failed(failure));
    }

    /// Appends a skipped item.
    pub fn push_skipped(&mut self, key: K, reason: impl Into<String>) {
        self.push(
            key,
            BatchOutcome::Skipped {
                reason: reason.into(),
            },
        );
    }

    /// Every record, in the order the items were handled.
    pub fn records(&self) -> &[BatchRecord<K, S, F>] {
        &self.records
    }

    /// Every record, in the order the items were handled.
    pub fn iter(&self) -> std::slice::Iter<'_, BatchRecord<K, S, F>> {
        self.records.iter()
    }

    /// How many items were handled.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether no item was handled.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The items that succeeded, with what each reported.
    pub fn succeeded(&self) -> impl Iterator<Item = (&K, &S)> {
        self.records.iter().filter_map(|r| match &r.outcome {
            BatchOutcome::Succeeded(success) => Some((&r.key, success)),
            _ => None,
        })
    }

    /// The items that failed, with what each reported.
    pub fn failed(&self) -> impl Iterator<Item = (&K, &F)> {
        self.records.iter().filter_map(|r| match &r.outcome {
            BatchOutcome::Failed(failure) => Some((&r.key, failure)),
            _ => None,
        })
    }

    /// The items that were skipped, with the reason.
    pub fn skipped(&self) -> impl Iterator<Item = (&K, &str)> {
        self.records.iter().filter_map(|r| match &r.outcome {
            BatchOutcome::Skipped { reason } => Some((&r.key, reason.as_str())),
            _ => None,
        })
    }

    /// How many items ended each way.
    pub fn counts(&self) -> BatchCounts {
        self.records
            .iter()
            .fold(BatchCounts::default(), |mut counts, r| {
                match r.outcome {
                    BatchOutcome::Succeeded(_) => counts.succeeded += 1,
                    BatchOutcome::Failed(_) => counts.failed += 1,
                    BatchOutcome::Skipped { .. } => counts.skipped += 1,
                }
                counts
            })
    }

    /// Whether any item failed. Skipped items are not failures.
    pub fn has_failures(&self) -> bool {
        self.records.iter().any(|r| r.outcome.is_failed())
    }

    /// The exit outcome of the batch: [`Outcome::Failures`] (exit 1) when any item failed, else
    /// [`Outcome::Success`] (exit 0); an empty or only-skipped batch is a success.
    pub fn outcome(&self) -> Outcome {
        Outcome::from_success(!self.has_failures())
    }
}

impl<K, S, F> IntoIterator for BatchReport<K, S, F> {
    type Item = BatchRecord<K, S, F>;
    type IntoIter = std::vec::IntoIter<BatchRecord<K, S, F>>;

    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter()
    }
}

impl<'a, K, S, F> IntoIterator for &'a BatchReport<K, S, F> {
    type Item = &'a BatchRecord<K, S, F>;
    type IntoIter = std::slice::Iter<'a, BatchRecord<K, S, F>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<K, S, F> FromIterator<BatchRecord<K, S, F>> for BatchReport<K, S, F> {
    fn from_iter<I: IntoIterator<Item = BatchRecord<K, S, F>>>(iter: I) -> Self {
        Self {
            records: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Report = BatchReport<String, u32, String>;

    fn mixed() -> Report {
        let mut report = Report::new();
        report.push_succeeded("a".into(), 1);
        report.push_failed("b".into(), "boom".into());
        report.push_skipped("c".into(), "not selected");
        report.push_succeeded("d".into(), 4);
        report
    }

    #[test]
    fn counts_each_outcome_and_totals_them() {
        let counts = mixed().counts();
        assert_eq!(
            counts,
            BatchCounts {
                succeeded: 2,
                failed: 1,
                skipped: 1
            }
        );
        assert_eq!(counts.total(), 4);
        assert_eq!(Report::new().counts(), BatchCounts::default());
    }

    #[test]
    fn iterators_keep_the_order_the_items_were_handled_in() {
        let report = mixed();
        assert_eq!(
            report.succeeded().collect::<Vec<_>>(),
            [(&"a".to_owned(), &1), (&"d".to_owned(), &4)]
        );
        assert_eq!(
            report.failed().collect::<Vec<_>>(),
            [(&"b".to_owned(), &"boom".to_owned())]
        );
        assert_eq!(
            report.skipped().collect::<Vec<_>>(),
            [(&"c".to_owned(), "not selected")]
        );
        let keys: Vec<&str> = report.records().iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["a", "b", "c", "d"]);
        assert_eq!(report.len(), 4);
    }

    #[test]
    fn all_success_is_exit_0() {
        let mut report = Report::new();
        report.push_succeeded("a".into(), 1);
        report.push_succeeded("b".into(), 2);
        assert!(!report.has_failures());
        assert_eq!(report.outcome(), Outcome::Success);
        assert_eq!(report.outcome().exit_code(), 0);
    }

    #[test]
    fn one_failure_among_successes_is_exit_1() {
        let report = mixed();
        assert!(report.has_failures());
        assert_eq!(report.outcome(), Outcome::Failures);
        assert_eq!(report.outcome().exit_code(), 1);
    }

    #[test]
    fn failures_without_any_success_are_exit_1() {
        let mut report = Report::new();
        report.push_failed("a".into(), "boom".into());
        report.push_failed("b".into(), "bang".into());
        assert_eq!(report.counts().succeeded, 0);
        assert!(report.has_failures());
        assert_eq!(report.outcome(), Outcome::Failures);
        assert_eq!(report.outcome().exit_code(), 1);

        report.push_skipped("c".into(), "not selected");
        assert_eq!(report.outcome(), Outcome::Failures);
    }

    #[test]
    fn only_skipped_or_empty_is_not_a_failure() {
        let mut report = Report::new();
        assert!(report.is_empty());
        assert_eq!(report.outcome(), Outcome::Success);
        report.push_skipped("a".into(), "nothing to do");
        assert!(!report.has_failures());
        assert_eq!(report.outcome(), Outcome::Success);
    }

    #[test]
    fn serde_json_round_trip_preserves_every_outcome() {
        let report = mixed();
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            json,
            r#"{"records":[{"key":"a","outcome":{"succeeded":1}},{"key":"b","outcome":{"failed":"boom"}},{"key":"c","outcome":{"skipped":{"reason":"not selected"}}},{"key":"d","outcome":{"succeeded":4}}]}"#
        );
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
    }

    #[test]
    fn collects_from_and_iterates_records() {
        let report: Report = mixed()
            .into_iter()
            .filter(|r| !r.outcome.is_failed())
            .collect();
        assert!(!report.has_failures());
        assert_eq!((&report).into_iter().count(), 3);
    }
}
