//! What a release or patch would contain: the model behind the CLI plan and the release hub's board.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use vdp_git::{Commit, Vcs};
use vdp_jira::{FixVersion, IssueTracker, Ticket, browse_url};

use crate::policy::{Classification, ReleasePolicy};
use crate::scan::{ScanNames, TicketKind, ViolationVia, compare_state, scan_source};
use crate::{ReleaseError, remote_ref};

/// A release cuts a new branch from the source; a patch adds tickets to an existing release branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AnalysisKind {
    Release,
    Patch,
}

/// What to analyse. Refs are remote refs (`origin/dev`); see [`remote_ref`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnalysisRequest {
    pub kind: AnalysisKind,
    /// Where the changes come from (`origin/dev`).
    pub source: String,
    /// What they are compared with: `origin/main` for a release, the release branch for a patch.
    pub compare: String,
    /// The target DROP, e.g. `2026_DROP25`.
    pub fix_version: String,
}

/// A commit, as shown to people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommitInfo {
    pub hash: String,
    pub short: String,
    pub author: String,
    /// Committer time, seconds since the epoch.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub timestamp: i64,
    pub subject: String,
    /// Position on the source mainline since the merge-base (0 = oldest); `None` for commits elsewhere.
    pub position: Option<u32>,
}

impl CommitInfo {
    /// Summarises `commit`, with its `position` on the source mainline when it has one.
    pub fn from_commit(commit: &Commit, position: Option<usize>) -> Self {
        Self {
            hash: commit.hash.clone(),
            short: commit.short().to_owned(),
            author: commit.author.clone(),
            timestamp: commit.timestamp,
            subject: commit.message.lines().next().unwrap_or_default().to_owned(),
            position: position.and_then(|p| u32::try_from(p).ok()),
        }
    }
}

/// The Jira side of a ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JiraInfo {
    pub summary: String,
    pub status: String,
    pub fix_versions: Vec<String>,
    pub assignee: Option<String>,
    pub reporter: Option<String>,
    pub components: Vec<String>,
    pub sprint: Option<String>,
    pub url: String,
}

impl JiraInfo {
    fn from_ticket(ticket: &Ticket, base_url: &str) -> Self {
        Self {
            summary: ticket.summary.clone(),
            status: ticket.status.clone(),
            fix_versions: ticket.fix_versions.clone(),
            assignee: ticket.assignee.clone(),
            reporter: ticket.reporter.clone(),
            components: ticket.components.clone(),
            sprint: ticket.sprint.clone(),
            url: browse_url(base_url, &ticket.key),
        }
    }
}

/// One ticket (or hotfix/bugfix branch) and everything known about it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "flat data for the release board"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TicketEntry {
    pub id: String,
    pub kind: TicketKind,
    /// Its merges on the source mainline since the merge-base, oldest first.
    pub merges: Vec<CommitInfo>,
    /// The revert that has it reverted on the compare branch now (its latest change there is a revert).
    pub reverted_on_compare: Option<CommitInfo>,
    /// Re-enabled on the compare branch (a revert of its revert is its latest change there).
    pub reenabled_on_compare: Option<CommitInfo>,
    /// On the compare branch too (merged, picked or re-enabled, and not reverted there).
    pub already_on_compare: bool,
    /// Other ticket ids its merges name, or the commits they bring (e.g. INTEL-27178 inside INTEL-27128).
    pub also_mentions: Vec<String>,
    pub jira: Option<JiraInfo>,
    pub jira_error: Option<String>,
    pub classification: Classification,
    /// Targets an emergency drop.
    pub emergency: bool,
    /// Whether it can be selected: a release reverts merges (so it needs some); a patch adds what the
    /// release branch lacks.
    pub selectable: bool,
    /// In the default selection (reverted for a release, included for a patch).
    pub selected_by_default: bool,
}

/// A revert commit on the compare branch that the source branch does not have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CompareRevert {
    pub commit: CommitInfo,
    pub tickets: Vec<String>,
}

/// A revert that reached the source branch, which must never have one: shown, never repeated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DevViolation {
    pub commit: CommitInfo,
    pub tickets: Vec<String>,
    /// A re-apply (revert of a revert) rather than a revert.
    pub reapply: bool,
    pub via: ViolationVia,
}

/// The analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Analysis {
    pub request: AnalysisRequest,
    pub merge_base: String,
    pub source_head: String,
    pub compare_head: String,
    /// In mainline order of their first merge; tickets only reverted on the compare branch come last.
    pub tickets: Vec<TicketEntry>,
    /// Direct commits on the source mainline without a ticket.
    pub uncoupled: Vec<CommitInfo>,
    /// Reverts on the compare branch that leave a ticket reverted there (the latest per ticket).
    pub compare_reverts: Vec<CompareRevert>,
    /// Reverts found on the source branch (it must have none).
    pub dev_violations: Vec<DevViolation>,
    /// Fixes the compare branch has that the source lacks: a new promotion would drop them from main (release),
    /// or they live only on the release branch (patch). The developers bring them to the source themselves.
    pub release_only: Vec<crate::ReleaseOnly>,
    /// Tickets without a DROP fix version stay in (`--keep-empty-fixversions`); otherwise they are reverted,
    /// including an earlier release's revert of one, which is carried over.
    #[serde(default)]
    pub keep_empty: bool,
}

impl crate::TicketEntry {
    /// Out of this release by policy: reverted, or without a fix version (unless those are kept).
    pub fn reverted_by_policy(&self, keep_empty: bool) -> bool {
        match self.classification {
            Classification::Revert(_) => true,
            Classification::NoFixVersion => !keep_empty,
            Classification::Keep(_) => false,
        }
    }
}

impl Analysis {
    /// The ticket entry with this `id`, if the analysis has it.
    pub fn ticket(&self, id: &str) -> Option<&TicketEntry> {
        self.tickets.iter().find(|t| t.id == id)
    }

    /// The tickets selected by default.
    pub fn default_selection(&self) -> BTreeSet<String> {
        self.tickets
            .iter()
            .filter(|t| t.selected_by_default)
            .map(|t| t.id.clone())
            .collect()
    }

    /// Tickets whose Jira data is missing.
    pub fn jira_errors(&self) -> Vec<(&str, &str)> {
        self.tickets
            .iter()
            .filter_map(|t| t.jira_error.as_deref().map(|e| (t.id.as_str(), e)))
            .collect()
    }

    /// Checks a selection: every id exists and is selectable.
    ///
    /// # Errors
    ///
    /// [`ReleaseError::Invalid`] when an id of `selection` is not in the analysis or is not selectable.
    pub fn validate_selection(&self, selection: &BTreeSet<String>) -> Result<(), ReleaseError> {
        for id in selection {
            match self.ticket(id) {
                None => {
                    return Err(ReleaseError::Invalid(format!(
                        "{id} is not in this analysis"
                    )));
                }
                Some(t) if !t.selectable => {
                    return Err(ReleaseError::Invalid(format!(
                        "{id} cannot be selected here"
                    )));
                }
                Some(_) => {}
            }
        }
        Ok(())
    }
}

/// Progress while analysing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Step(String),
    /// A Jira lookup: `(done, total, key)`.
    Jira(usize, usize, String),
}

/// Builds analyses.
pub struct Analyser<'a> {
    pub vcs: &'a dyn Vcs,
    /// A patch's compare branch must be a release branch.
    pub rules: &'a crate::BranchRules,
    /// `None`: Jira is not configured; every ticket gets a `jira_error`.
    pub tracker: Option<&'a dyn IssueTracker>,
    pub jira_base_url: String,
    pub ticket_prefix: String,
    pub excluded: Vec<String>,
    /// [`Classification::NoFixVersion`] is kept instead of reverted by default (the CLI's
    /// `--keep-empty-fixversions`).
    pub keep_empty: bool,
    pub progress: &'a dyn Fn(Progress),
}

impl std::fmt::Debug for Analyser<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Analyser")
            .field("vcs", &self.vcs)
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Draft {
    kind: Option<TicketKind>,
    merges: Vec<CommitInfo>,
    reverted: Option<CommitInfo>,
    reenabled: Option<CommitInfo>,
    also: Vec<String>,
}

/// Both sides' commits since the merge-base, scanned.
struct History {
    mainline: Vec<Commit>,
    all: Vec<Commit>,
    compare_commits: Vec<Commit>,
    scan: crate::SourceScan,
    state: crate::CompareState,
}

impl History {
    fn info(&self, hash: &str, position: Option<usize>) -> Option<CommitInfo> {
        self.mainline
            .iter()
            .chain(&self.all)
            .chain(&self.compare_commits)
            .find(|c| c.hash == hash)
            .map(|c| CommitInfo::from_commit(c, position))
    }

    /// Tickets in order of first appearance on the mainline, then those only changed on the compare branch.
    fn drafts(&self) -> Result<(Vec<String>, BTreeMap<String, Draft>), ReleaseError> {
        let mut order: Vec<String> = Vec::new();
        let mut drafts: BTreeMap<String, Draft> = BTreeMap::new();
        for merge in &self.scan.merged {
            let commit = self
                .info(&merge.commit, Some(merge.position))
                .ok_or_else(|| ReleaseError::Invalid(format!("unknown commit {}", merge.commit)))?;
            let entry = draft(&mut drafts, &mut order, &merge.ticket);
            entry.kind = Some(merge.kind);
            entry.merges.push(commit);
        }
        for (ticket, also) in &self.scan.also_mentions {
            draft(&mut drafts, &mut order, ticket).also = also.iter().cloned().collect();
        }
        for (ticket, hash) in &self.state.reverted {
            draft(&mut drafts, &mut order, ticket).reverted = self.info(hash, None);
        }
        for (ticket, hash) in &self.state.reenabled {
            draft(&mut drafts, &mut order, ticket).reenabled = self.info(hash, None);
        }
        Ok((order, drafts))
    }

    fn violations(&self) -> Vec<DevViolation> {
        self.scan
            .violations
            .iter()
            .filter_map(|v| {
                Some(DevViolation {
                    commit: self.info(&v.commit, None)?,
                    tickets: v.tickets.clone(),
                    reapply: v.reapply,
                    via: v.via.clone(),
                })
            })
            .collect()
    }
}

/// Reverts on the compare branch that leave a ticket reverted there (the latest per ticket), per commit.
fn group_reverts(commits: &[Commit], state: &crate::CompareState) -> Vec<CompareRevert> {
    commits
        .iter()
        .filter_map(|commit| {
            let tickets: Vec<String> = state
                .reverted
                .iter()
                .filter(|(_, hash)| **hash == commit.hash)
                .map(|(ticket, _)| ticket.clone())
                .collect();
            (!tickets.is_empty()).then(|| CompareRevert {
                commit: CommitInfo::from_commit(commit, None),
                tickets,
            })
        })
        .collect()
}

/// The draft of a ticket, created (and put in order) on first sight.
fn draft<'d>(
    drafts: &'d mut BTreeMap<String, Draft>,
    order: &mut Vec<String>,
    id: &str,
) -> &'d mut Draft {
    drafts.entry(id.to_owned()).or_insert_with(|| {
        order.push(id.to_owned());
        Draft::default()
    })
}

impl Analyser<'_> {
    /// Analyses the current refs (fetch first).
    ///
    /// # Errors
    ///
    /// [`ReleaseError::Invalid`] when the fix version does not parse or, for a patch, the compare branch is not a release
    /// branch; [`ReleaseError::Git`] when a ref cannot be resolved or the history cannot be read.
    pub fn analyse(&self, request: &AnalysisRequest) -> Result<Analysis, ReleaseError> {
        let target: FixVersion = request.fix_version.parse().map_err(ReleaseError::Invalid)?;
        if request.kind == AnalysisKind::Patch {
            self.rules.release_target(&request.compare)?;
        }
        let policy = ReleasePolicy {
            target,
            keep_empty: self.keep_empty,
            excluded: self.excluded.clone(),
        };
        let source = remote_ref(&request.source);
        let compare = remote_ref(&request.compare);
        let source_head = self.vcs.rev_parse(&source)?;
        let compare_head = self.vcs.rev_parse(&compare)?;
        let merge_base = self.vcs.merge_base(&compare, &source)?;
        (self.progress)(Progress::Step(format!(
            "merge-base of {compare} and {source}: {}",
            &merge_base[..merge_base.len().min(12)]
        )));
        let history = self.history(&merge_base, &source, &compare)?;
        let (order, mut drafts) = history.drafts()?;
        let compare_reverts = group_reverts(&history.compare_commits, &history.state);

        let jira_total = order
            .iter()
            .filter(|id| drafts[*id].kind.unwrap_or(TicketKind::Jira) == TicketKind::Jira)
            .count();
        let mut jira_done = 0;
        let mut tickets = Vec::with_capacity(order.len());
        for id in order {
            let draft = drafts.remove(&id).unwrap_or_default();
            if draft.kind.unwrap_or(TicketKind::Jira) == TicketKind::Jira {
                jira_done += 1;
                (self.progress)(Progress::Jira(jira_done, jira_total, id.clone()));
            }
            let already_on_compare = (history.state.present.contains(&id)
                || draft.reenabled.is_some())
                && draft.reverted.is_none();
            tickets.push(self.entry(&policy, request.kind, id, draft, already_on_compare));
        }

        let uncoupled = history
            .scan
            .uncoupled
            .iter()
            .map(|&position| CommitInfo::from_commit(&history.mainline[position], Some(position)))
            .collect();
        let release_only = crate::release_only(
            self.vcs,
            &crate::ReleaseOnlyRequest {
                source: &source,
                head: &compare,
                ticket_prefix: &self.ticket_prefix,
            },
        )?;
        let dev_violations = history.violations();
        Ok(Analysis {
            request: AnalysisRequest {
                kind: request.kind,
                source,
                compare,
                fix_version: request.fix_version.clone(),
            },
            merge_base,
            source_head,
            compare_head,
            tickets,
            uncoupled,
            compare_reverts,
            dev_violations,
            release_only,
            keep_empty: self.keep_empty,
        })
    }

    /// The commits of both sides since the merge-base, scanned.
    fn history(
        &self,
        merge_base: &str,
        source: &str,
        compare: &str,
    ) -> Result<History, ReleaseError> {
        let prefix = &self.ticket_prefix;
        let mainline = self.vcs.log(merge_base, source, true)?;
        let all = self.vcs.log(merge_base, source, false)?;
        let names = ScanNames {
            source: self.rules.source.clone(),
            main: self.rules.main.clone(),
            release_prefix: self.rules.prefix.clone(),
        };
        let scan = scan_source(crate::SourceScanInput {
            mainline: &mainline,
            all: &all,
            prefix,
            names: &names,
        });
        // Everything the compare branch has since the merge-base, through any parent (main gets its tickets
        // through release-branch merges), with each ticket's net revert state.
        let compare_commits = self.vcs.log(merge_base, compare, false)?;
        let state = compare_state(&compare_commits, prefix);
        (self.progress)(Progress::Step(format!(
            "{} merges with a ticket on {source}, {} ticket(s) reverted on {compare}",
            scan.merged.len(),
            state.reverted.len()
        )));
        if !scan.violations.is_empty() {
            (self.progress)(Progress::Step(format!(
                "{} revert(s) found on {source}, which must never have one",
                scan.violations.len()
            )));
        }
        Ok(History {
            mainline,
            all,
            compare_commits,
            scan,
            state,
        })
    }
    /// One ticket: Jira data, classification and selection defaults.
    fn entry(
        &self,
        policy: &ReleasePolicy,
        kind: AnalysisKind,
        id: String,
        draft: Draft,
        already_on_compare: bool,
    ) -> TicketEntry {
        let ticket_kind = draft.kind.unwrap_or(TicketKind::Jira);
        let (jira, jira_error, classification, emergency) = if ticket_kind == TicketKind::Jira {
            match self.tracker.map(|t| t.ticket(&id)) {
                Some(Ok(ticket)) => (
                    Some(JiraInfo::from_ticket(&ticket, &self.jira_base_url)),
                    None,
                    policy.classify(&ticket),
                    !ticket.emergency_drops().is_empty(),
                ),
                Some(Err(error)) => (
                    None,
                    Some(error.to_string()),
                    Classification::NoFixVersion,
                    false,
                ),
                None => (
                    None,
                    Some("Jira is not configured".to_owned()),
                    Classification::NoFixVersion,
                    false,
                ),
            }
        } else {
            let reason = match ticket_kind {
                TicketKind::Hotfix => "hotfix branch: always included",
                TicketKind::Bugfix => "bugfix branch: always included",
                _ => "branch merged without a ticket: included unless you revert it",
            };
            (None, None, Classification::Keep(reason.to_owned()), false)
        };
        let (selectable, selected_by_default) = match kind {
            AnalysisKind::Release => {
                let selectable = !draft.merges.is_empty();
                let revert = match &classification {
                    Classification::Keep(_) => false,
                    Classification::Revert(_) => true,
                    Classification::NoFixVersion => !self.keep_empty,
                };
                (selectable, selectable && revert)
            }
            AnalysisKind::Patch => {
                let selectable =
                    !already_on_compare && (!draft.merges.is_empty() || draft.reverted.is_some());
                let wanted = emergency || (draft.reverted.is_some() && classification.is_keep());
                (selectable, selectable && wanted)
            }
        };
        TicketEntry {
            id,
            kind: ticket_kind,
            merges: draft.merges,
            reverted_on_compare: draft.reverted,
            reenabled_on_compare: draft.reenabled,
            already_on_compare,
            also_mentions: draft.also,
            jira,
            jira_error,
            classification,
            emergency,
            selectable,
            selected_by_default,
        }
    }
}
