//! Conflicts during a release or patch, put to the release manager: what conflicts, which tickets touched those
//! files, and selection changes that avoid the conflict.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vdp_git::{ApplyOptions, Choice, Conflict, ConflictKind, OnConflict, Operation, Strategy, Vcs};

use crate::analysis::{Analysis, AnalysisKind};
use crate::flow::Step;

/// How a run handles conflicts (`resolve: None` with `Pause` aborts on a conflict).
#[derive(Clone, Copy, Default)]
pub struct Conflicts<'a> {
    pub options: ApplyOptions,
    /// Asks for a decision (the hub: blocks until the release manager answers in the UI).
    pub resolve: Option<&'a dyn Fn(&ConflictReport) -> Resolution>,
}

impl std::fmt::Debug for Conflicts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conflicts")
            .field("options", &self.options)
            .field("resolve", &self.resolve.is_some())
            .finish()
    }
}

/// The run options the API offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum OnConflictChoice {
    /// Stop and ask.
    #[default]
    Pause,
    /// Resolve for the change being applied, automatically.
    Theirs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StrategyChoice {
    /// Conflicting hunks take the change being applied; only whole-file conflicts are left.
    #[default]
    TheirsHunks,
    /// Every conflicting hunk is a conflict.
    Strict,
}

pub fn apply_options(on_conflict: OnConflictChoice, strategy: StrategyChoice) -> ApplyOptions {
    ApplyOptions {
        on_conflict: match on_conflict {
            OnConflictChoice::Pause => OnConflict::Pause,
            OnConflictChoice::Theirs => OnConflict::Theirs,
        },
        strategy: match strategy {
            StrategyChoice::TheirsHunks => Strategy::TheirsHunks,
            StrategyChoice::Strict => Strategy::Strict,
        },
    }
}

/// One conflicting file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConflictFile {
    pub path: String,
    /// `content`, `change_deletes`, `branch_deleted`, `both_added` or `other`.
    pub kind: String,
    /// Tickets whose merges on the source changed this file since the merge-base.
    pub touched_by: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SuggestedAction {
    /// Revert this kept ticket too (release): it builds on the one being reverted.
    AlsoRevert,
    /// Keep the ticket being reverted instead (release).
    KeepInstead,
    /// Add this ticket to the patch too: the one being picked builds on it.
    AlsoAdd,
}

/// A selection change that avoids the conflict (applied by a new analysis-preview round).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Suggestion {
    pub ticket: String,
    pub action: SuggestedAction,
    pub reason: String,
}

/// A conflict waiting for a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConflictReport {
    /// What stopped: a step's description, or the promotion merge.
    pub title: String,
    /// The release or patch step (none for a promotion merge).
    pub step: Option<Step>,
    /// `revert`, `cherry-pick` or `merge`.
    pub operation: String,
    pub files: Vec<ConflictFile>,
    pub suggestions: Vec<Suggestion>,
}

/// The decision for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FileChoice {
    /// The version of the change being applied (or its deletion).
    Change,
    /// The branch's version: the file stays as it is.
    Branch,
    Delete,
}

impl From<FileChoice> for Choice {
    fn from(choice: FileChoice) -> Self {
        match choice {
            FileChoice::Change => Self::Change,
            FileChoice::Branch => Self::Branch,
            FileChoice::Delete => Self::Delete,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ResolutionAction {
    /// Apply the file choices and commit the step.
    Continue,
    /// Leave this step out (the ticket stays as it is on the branch).
    SkipStep,
    /// Stop the whole run; nothing is pushed.
    Abort,
}

/// The release manager's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Resolution {
    #[serde(default)]
    pub choices: BTreeMap<String, FileChoice>,
    pub action: ResolutionAction,
}

impl Resolution {
    pub fn abort() -> Self {
        Self {
            choices: BTreeMap::new(),
            action: ResolutionAction::Abort,
        }
    }
}

fn kind_name(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::Content => "content",
        ConflictKind::ChangeDeletes => "change_deletes",
        ConflictKind::BranchDeleted => "branch_deleted",
        ConflictKind::BothAdded => "both_added",
        ConflictKind::Other => "other",
    }
}

fn step_tickets(step: &Step) -> Vec<String> {
    match step {
        Step::Revert { ticket, .. } | Step::ReEnable { ticket, .. } | Step::Pick { ticket, .. } => {
            vec![ticket.clone()]
        }
        Step::CherryPick { tickets, .. } => tickets.clone(),
    }
}

/// What the release manager needs to decide: the files, the tickets that touched them, and selection
/// changes that avoid the conflict.
pub(crate) fn report(
    vcs: &dyn Vcs,
    analysis: &Analysis,
    selection: &BTreeSet<String>,
    step: &Step,
    conflict: &Conflict,
) -> ConflictReport {
    // Mainline merge commit → ticket.
    let by_merge: BTreeMap<&str, &str> = analysis
        .tickets
        .iter()
        .flat_map(|t| {
            t.merges
                .iter()
                .map(move |m| (m.hash.as_str(), t.id.as_str()))
        })
        .collect();
    let position = |ticket: &str| {
        analysis
            .ticket(ticket)
            .and_then(|t| t.merges.first())
            .and_then(|m| m.position)
    };
    let mine = step_tickets(step);
    let mut files = Vec::new();
    let mut suggestions: Vec<Suggestion> = Vec::new();
    for path in &conflict.paths {
        let touched: Vec<String> = vcs
            .touching(&analysis.merge_base, &analysis.source_head, &path.path)
            .unwrap_or_default()
            .iter()
            .filter_map(|hash| by_merge.get(hash.as_str()).map(|t| (*t).to_owned()))
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        for other in touched.iter().filter(|t| !mine.contains(t)) {
            let later = mine
                .iter()
                .filter_map(|m| position(m))
                .min()
                .zip(position(other))
                .is_some_and(|(me, them)| them > me);
            let suggestion = match (analysis.request.kind, step) {
                // Reverting a ticket that a later kept ticket builds on.
                (AnalysisKind::Release, Step::Revert { ticket, .. })
                    if later && !selection.contains(other) =>
                {
                    vec![
                        Suggestion {
                            ticket: other.clone(),
                            action: SuggestedAction::AlsoRevert,
                            reason: format!(
                                "{other} (kept) changed {} after {ticket}: revert it too",
                                path.path
                            ),
                        },
                        Suggestion {
                            ticket: ticket.clone(),
                            action: SuggestedAction::KeepInstead,
                            reason: format!("keep {ticket}, so {other} keeps what it builds on"),
                        },
                    ]
                }
                // Picking a ticket that builds on one the release branch lacks.
                (AnalysisKind::Patch, Step::Pick { ticket, .. })
                    if !later
                        && !selection.contains(other)
                        && analysis
                            .ticket(other)
                            .is_some_and(|t| t.selectable && !t.already_on_compare) =>
                {
                    vec![Suggestion {
                        ticket: other.clone(),
                        action: SuggestedAction::AlsoAdd,
                        reason: format!(
                            "{ticket} builds on {other} in {}, which the release branch lacks: add it too",
                            path.path
                        ),
                    }]
                }
                _ => Vec::new(),
            };
            for item in suggestion {
                if !suggestions
                    .iter()
                    .any(|s| s.ticket == item.ticket && s.action == item.action)
                {
                    suggestions.push(item);
                }
            }
        }
        files.push(ConflictFile {
            path: path.path.clone(),
            kind: kind_name(path.kind).to_owned(),
            touched_by: touched,
        });
    }
    ConflictReport {
        title: step.describe(),
        step: Some(step.clone()),
        operation: conflict.operation.command().to_owned(),
        files,
        suggestions,
    }
}

/// A conflict without an analysis (a promotion merge): the files only.
pub fn plain_report(title: &str, conflict: &Conflict) -> ConflictReport {
    ConflictReport {
        title: title.to_owned(),
        step: None,
        operation: conflict.operation.command().to_owned(),
        files: conflict
            .paths
            .iter()
            .map(|p| ConflictFile {
                path: p.path.clone(),
                kind: kind_name(p.kind).to_owned(),
                touched_by: Vec::new(),
            })
            .collect(),
        suggestions: Vec::new(),
    }
}

/// The git operation of a step.
pub(crate) fn operation(step: &Step) -> Operation {
    match step {
        Step::Revert { .. } | Step::ReEnable { .. } => Operation::Revert,
        Step::CherryPick { .. } | Step::Pick { .. } => Operation::CherryPick,
    }
}
