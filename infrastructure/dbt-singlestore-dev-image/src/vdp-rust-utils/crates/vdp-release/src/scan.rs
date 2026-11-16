//! Tickets in commit history.
//!
//! - The **source** (`dev`) is scanned along its mainline (first parent), oldest first. Every mainline merge
//!   is classified by its message and by the commits it brings: a ticket merge, a hotfix/bugfix branch, a
//!   ticketless branch merge (a `branch:<name>` pseudo-ticket), a GitLab revert MR, a back-merge of a release
//!   branch or main, or a sync merge of the source into itself.
//! - `dev` must never receive a revert. Any revert or re-apply found on it (directly, through a revert MR, or
//!   brought by a back-merge) is a [`RawViolation`]; it still updates the content state, so the release is
//!   built from what `dev` really contains.
//! - The **compare** branch (main or a release branch) legitimately holds reverts and re-enables (reverts of
//!   reverts); [`compare_state`] gives each ticket's *net* state there.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use vdp_git::Commit;

/// What kind of change a merge brought.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TicketKind {
    /// A Jira ticket (`INTEL-1234`).
    Jira,
    /// A `hotfix/<slug>` branch without a ticket: always kept.
    Hotfix,
    /// A `bugfix/<slug>` branch without a ticket: always kept.
    Bugfix,
    /// A merged branch without a ticket or hotfix/bugfix name (`branch:<name>`): kept by default, flagged.
    Branch,
}

/// A ticket brought in by a commit on the release's source branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedTicket {
    /// `INTEL-1234`, `hotfix/<slug>`, `bugfix/<slug>` or `branch:<name>`.
    pub ticket: String,
    pub kind: TicketKind,
    pub commit: String,
    pub author: String,
    pub timestamp: i64,
    /// Position of the mainline commit that brought it (0 = oldest).
    pub position: usize,
}

/// How a revert reached `dev`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "via", rename_all = "snake_case")]
pub enum ViolationVia {
    /// Committed on the mainline itself.
    Direct,
    /// Merged from a revert branch (GitLab's "Revert" button: `revert-<sha>`).
    RevertMerge { merge: String },
    /// Brought by merging a release branch or main into the source.
    BackMerge { merge: String, from: String },
}

/// A revert (or re-apply) found on the source branch, which must never have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawViolation {
    pub commit: String,
    pub tickets: Vec<String>,
    /// A re-apply (revert of a revert) rather than a revert.
    pub reapply: bool,
    pub via: ViolationVia,
    /// Position of the mainline commit through which it arrived.
    pub position: usize,
}

/// What the source mainline holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceScan {
    /// Ticket merges still in effect, in mainline order.
    pub merged: Vec<MergedTicket>,
    pub violations: Vec<RawViolation>,
    /// Other ticket ids named by a ticket's merges or the commits they bring.
    pub also_mentions: BTreeMap<String, BTreeSet<String>>,
    /// Positions of direct mainline commits without a ticket.
    pub uncoupled: Vec<usize>,
}

/// Branch names the scan treats specially.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanNames {
    /// The source branch (`dev`): merging it into itself is a sync merge.
    pub source: String,
    /// `main`: merging it into the source is a back-merge.
    pub main: String,
    /// `release/`: merging such a branch into the source is a back-merge.
    pub release_prefix: String,
}

/// Every `<PREFIX>-<digits>` in `text` (case-insensitive), upper-cased, in order, deduplicated.
pub(crate) fn ticket_ids(text: &str, prefix: &str) -> Vec<String> {
    let upper = text.to_uppercase();
    let marker = format!("{}-", prefix.to_uppercase());
    let mut ids = Vec::new();
    let mut rest = upper.as_str();
    while let Some(start) = rest.find(&marker) {
        let after = &rest[start + marker.len()..];
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        let preceded_by_word = rest[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        if !digits.is_empty() && !preceded_by_word {
            let id = format!("{marker}{digits}");
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        rest = &after[digits.len()..];
    }
    ids
}

/// The first `hotfix/<slug>` or `bugfix/<slug>` in a message (as `release_service.py` matched it).
pub(crate) fn branch_change(message: &str) -> Option<(TicketKind, String)> {
    let bytes = message.as_bytes();
    let mut found: Option<(usize, TicketKind)> = None;
    for (word, kind) in [
        ("hotfix/", TicketKind::Hotfix),
        ("bugfix/", TicketKind::Bugfix),
    ] {
        let position = bytes
            .windows(word.len())
            .position(|window| window.eq_ignore_ascii_case(word.as_bytes()));
        if let Some(position) = position
            && found.is_none_or(|(best, _)| position < best)
        {
            found = Some((position, kind));
        }
    }
    let (start, kind) = found?;
    let slug: String = message[start + 7..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect();
    let slug = slug.trim_end_matches('.');
    if slug.is_empty() {
        return None;
    }
    let name = if kind == TicketKind::Hotfix {
        "hotfix"
    } else {
        "bugfix"
    };
    Some((kind, format!("{name}/{slug}")))
}

/// The branch a merge message names: `Merge branch 'x' into 'dev'`, `Merge remote-tracking branch
/// 'origin/x'`, `Merge branch 'x' from repo into dev` (also inside a revert's quoted subject).
pub(crate) fn merge_branch_name(message: &str) -> Option<String> {
    let subject = message.lines().next().unwrap_or_default();
    let start = subject.find("Merge branch '").map(|i| i + 14).or_else(|| {
        subject
            .find("Merge remote-tracking branch '")
            .map(|i| i + 30)
    })?;
    let rest = &subject[start..];
    let name = &rest[..rest.find('\'')?];
    let name = name.strip_prefix("origin/").unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The commit a revert names in git's own trailer (`This reverts commit <sha>.`).
fn reverted_commit(message: &str) -> Option<&str> {
    let start = message.find("This reverts commit ")? + 20;
    let rest = &message[start..];
    let hash = &rest[..rest
        .find(|c: char| !c.is_ascii_hexdigit())
        .unwrap_or(rest.len())];
    (hash.len() >= 7).then_some(hash)
}

/// 0 = not a revert, 1 = a revert, 2 = a revert of a revert (a re-apply), and so on. From the subject's
/// `Revert "` nesting; a revert with a custom subject counts through its trailer (one level more than the
/// commit it names, when that commit is known). Commits that merely mention "revert" are not reverts.
pub(crate) fn revert_depth(commit: &Commit, known: &HashMap<&str, &Commit>) -> usize {
    revert_depth_bounded(commit, known, 16)
}

/// `Revert "…"` adds one level, `Reapply "…"` (newer git's name for a revert of a revert) two; returns the
/// depth and the innermost subject.
fn quoted_depth(subject: &str) -> (usize, &str) {
    let mut depth = 0;
    let mut rest = subject;
    loop {
        if let Some(inner) = rest.strip_prefix("Revert \"") {
            depth += 1;
            rest = inner;
        } else if let Some(inner) = rest.strip_prefix("Reapply \"") {
            depth += 2;
            rest = inner;
        } else {
            return (depth, rest);
        }
    }
}

fn revert_depth_bounded(commit: &Commit, known: &HashMap<&str, &Commit>, budget: usize) -> usize {
    let subject = commit.message.lines().next().unwrap_or_default();
    let (depth, _) = quoted_depth(subject);
    let mut depth = depth;
    // A hand-written `Revert INTEL-6 hotfix` (no quotes, no trailer) is a revert too.
    if depth == 0 && subject.to_lowercase().starts_with("revert ") {
        depth = 1;
    }
    if depth > 0 || budget == 0 {
        return depth;
    }
    match reverted_commit(&commit.message) {
        Some(target) => {
            1 + known
                .iter()
                .find(|(hash, _)| hash.starts_with(target))
                .map_or(0, |(_, t)| revert_depth_bounded(t, known, budget - 1))
        }
        None => 0,
    }
}

/// What a revert commit targets: its ticket (first id), else its hotfix/bugfix branch, else the branch of the
/// merge it reverts (`branch:<name>`).
fn revert_target(message: &str, prefix: &str) -> Option<String> {
    ticket_ids(message, prefix)
        .into_iter()
        .next()
        .or_else(|| branch_change(message).map(|(_, id)| id))
        .or_else(|| merge_branch_name(&strip_reverts(message)).map(|b| format!("branch:{b}")))
}

fn strip_reverts(message: &str) -> String {
    let subject = message.lines().next().unwrap_or_default();
    quoted_depth(subject).1.trim_end_matches('"').to_owned()
}

/// Commits a merge brings: reachable from its second (and further) parents within `known`, without crossing
/// into the mainline. Oldest first, as in `known_order`.
fn brought<'a>(
    merge: &Commit,
    known: &HashMap<&str, &'a Commit>,
    mainline: &HashSet<&str>,
    known_order: &HashMap<&str, usize>,
) -> Vec<&'a Commit> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = merge.parents.iter().skip(1).map(String::as_str).collect();
    let mut found: Vec<&'a Commit> = Vec::new();
    while let Some(hash) = stack.pop() {
        if mainline.contains(hash) || !seen.insert(hash) {
            continue;
        }
        let Some(commit) = known.get(hash) else {
            continue;
        };
        found.push(commit);
        stack.extend(commit.parents.iter().map(String::as_str));
    }
    found.sort_by_key(|c| known_order.get(c.hash.as_str()).copied().unwrap_or(0));
    found
}

struct Scanner<'a> {
    prefix: &'a str,
    names: &'a ScanNames,
    known: HashMap<&'a str, &'a Commit>,
    order: HashMap<&'a str, usize>,
    mainline: HashSet<&'a str>,
    out: SourceScan,
}

impl<'a> Scanner<'a> {
    fn activate(&mut self, ticket: String, kind: TicketKind, commit: &Commit, position: usize) {
        self.out.merged.push(MergedTicket {
            ticket,
            kind,
            commit: commit.hash.clone(),
            author: commit.author.clone(),
            timestamp: commit.timestamp,
            position,
        });
    }

    /// A revert (odd depth) cancels the ticket's earlier merges; a re-apply (even depth) brings it back. On
    /// the source either is a violation.
    fn revert(&mut self, commit: &Commit, depth: usize, via: ViolationVia, position: usize) {
        let Some(target) = revert_target(&commit.message, self.prefix) else {
            return;
        };
        let reapply = depth.is_multiple_of(2);
        if reapply {
            let kind = ticket_kind(&target);
            self.activate(target.clone(), kind, commit, position);
        } else {
            self.out.merged.retain(|t| t.ticket != target);
        }
        self.out.violations.push(RawViolation {
            commit: commit.hash.clone(),
            tickets: vec![target],
            reapply,
            via,
            position,
        });
    }

    fn visit(&mut self, commit: &'a Commit, position: usize, nested: bool) {
        if commit.parents.is_empty() {
            return;
        }
        let depth = revert_depth(commit, &self.known);
        if !commit.is_merge() {
            if depth > 0 {
                self.revert(commit, depth, ViolationVia::Direct, position);
            } else if let Some((kind, ticket)) = ticket_of(&commit.message, self.prefix) {
                self.activate(ticket, kind, commit, position);
            } else if !nested {
                self.out.uncoupled.push(position);
            }
            return;
        }
        let brought = brought(commit, &self.known, &self.mainline, &self.order);
        let name = merge_branch_name(&commit.message);
        let names = self.names;
        let back_merge = name.as_deref().filter(|n| {
            (!names.main.is_empty() && *n == names.main)
                || (!names.release_prefix.is_empty() && n.starts_with(&names.release_prefix))
        });
        if let Some(from) = back_merge {
            let from = from.to_owned();
            for inner in &brought {
                let depth = revert_depth(inner, &self.known);
                if depth > 0 && !inner.is_merge() {
                    let via = ViolationVia::BackMerge {
                        merge: commit.hash.clone(),
                        from: from.clone(),
                    };
                    self.revert(inner, depth, via, position);
                }
            }
            return;
        }
        if !nested && name.as_deref() == Some(names.source.as_str()) && !names.source.is_empty() {
            // A sync merge of the source into itself: what it brings is mainline work.
            for inner in brought {
                self.visit(inner, position, true);
            }
            return;
        }
        let changes: Vec<&&Commit> = brought.iter().filter(|c| !c.is_merge()).collect();
        let all_reverts =
            !changes.is_empty() && changes.iter().all(|c| revert_depth(c, &self.known) > 0);
        if all_reverts {
            for inner in changes {
                let depth = revert_depth(inner, &self.known);
                let via = ViolationVia::RevertMerge {
                    merge: commit.hash.clone(),
                };
                self.revert(inner, depth, via, position);
            }
            return;
        }
        let (kind, ticket) = ticket_of(&commit.message, self.prefix).unwrap_or_else(|| {
            let branch = name.unwrap_or_else(|| commit.short().to_owned());
            (TicketKind::Branch, format!("branch:{branch}"))
        });
        let mut also: BTreeSet<String> = ticket_ids(&commit.message, self.prefix)
            .into_iter()
            .collect();
        for inner in &brought {
            if revert_depth(inner, &self.known) == 0 {
                also.extend(ticket_ids(&inner.message, self.prefix));
            }
        }
        also.remove(&ticket);
        if !also.is_empty() {
            self.out
                .also_mentions
                .entry(ticket.clone())
                .or_default()
                .extend(also);
        }
        self.activate(ticket, kind, commit, position);
    }
}

/// The ticket a (non-revert) commit belongs to: its first ticket id, else its hotfix/bugfix branch.
fn ticket_of(message: &str, prefix: &str) -> Option<(TicketKind, String)> {
    ticket_ids(message, prefix)
        .into_iter()
        .next()
        .map(|id| (TicketKind::Jira, id))
        .or_else(|| branch_change(message))
}

fn ticket_kind(id: &str) -> TicketKind {
    if id.starts_with("hotfix/") {
        TicketKind::Hotfix
    } else if id.starts_with("bugfix/") {
        TicketKind::Bugfix
    } else if id.starts_with("branch:") {
        TicketKind::Branch
    } else {
        TicketKind::Jira
    }
}

/// Scans the source: `mainline` is `log --first-parent merge_base..source`, `all` is `log merge_base..source`
/// (both oldest first).
pub fn scan_source(
    mainline: &[Commit],
    all: &[Commit],
    prefix: &str,
    names: &ScanNames,
) -> SourceScan {
    let mut scanner = Scanner {
        prefix,
        names,
        known: all
            .iter()
            .chain(mainline)
            .map(|c| (c.hash.as_str(), c))
            .collect(),
        order: all
            .iter()
            .enumerate()
            .map(|(i, c)| (c.hash.as_str(), i))
            .collect(),
        mainline: mainline.iter().map(|c| c.hash.as_str()).collect(),
        out: SourceScan::default(),
    };
    for (position, commit) in mainline.iter().enumerate() {
        scanner.visit(commit, position, false);
    }
    scanner.out
}

/// The tickets merged by `commits` (a mainline, oldest first), without the merge contents: see [`scan_source`].
pub fn merged_tickets(commits: &[Commit], prefix: &str) -> Vec<MergedTicket> {
    scan_source(commits, commits, prefix, &ScanNames::default()).merged
}

/// Each ticket's net state on the compare branch since the merge-base.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompareState {
    /// Tickets whose latest change there is a revert: the commit that reverted them.
    pub reverted: BTreeMap<String, String>,
    /// Tickets whose latest change there re-enabled them (a revert of a revert): that commit.
    pub reenabled: BTreeMap<String, String>,
    /// Tickets named by a non-revert commit there (merged, cherry-picked or committed on it).
    pub present: BTreeSet<String>,
}

/// `commits` is `log merge_base..compare` (any parent, oldest first). A revert commit counts for every ticket
/// it names; reverts and re-enables flip a ticket's state in order, so a re-enabled ticket is not reverted.
pub fn compare_state(commits: &[Commit], prefix: &str) -> CompareState {
    let known: HashMap<&str, &Commit> = commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    let mut state = CompareState::default();
    for commit in commits {
        let depth = revert_depth(commit, &known);
        if depth == 0 {
            state.present.extend(ticket_ids(&commit.message, prefix));
            if let Some((_, id)) = branch_change(&commit.message) {
                state.present.insert(id);
            }
            continue;
        }
        let mut targets = ticket_ids(&strip_reverts(&commit.message), prefix);
        if targets.is_empty() {
            targets.extend(revert_target(&commit.message, prefix));
        }
        for ticket in targets {
            if depth.is_multiple_of(2) {
                state.reverted.remove(&ticket);
                state.reenabled.insert(ticket, commit.hash.clone());
            } else {
                state.reenabled.remove(&ticket);
                state.reverted.insert(ticket, commit.hash.clone());
            }
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, message: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: hash.into(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            author: "a".into(),
            timestamp: 0,
            message: message.into(),
        }
    }

    /// A merge on the mainline, its first parent the previous mainline commit.
    fn merge(hash: &str, message: &str, prev: &str, side: &str) -> Commit {
        commit(hash, message, &[prev, side])
    }

    fn names() -> ScanNames {
        ScanNames {
            source: "dev".into(),
            main: "main".into(),
            release_prefix: "release/".into(),
        }
    }

    fn ids(scan: &SourceScan) -> Vec<(&str, &str)> {
        scan.merged
            .iter()
            .map(|t| (t.ticket.as_str(), t.commit.as_str()))
            .collect()
    }

    #[test]
    fn ticket_ids_are_whole_and_untruncated() {
        assert_eq!(
            ticket_ids(
                "Merge branch 'feature/intel-123456-x' fixes INTEL-7, XINTEL-9",
                "INTEL"
            ),
            ["INTEL-123456", "INTEL-7"]
        );
    }

    #[test]
    fn revert_depth_from_subject_and_trailer() {
        let known = HashMap::new();
        let depth = |m: &str| revert_depth(&commit("x", m, &["p"]), &known);
        assert_eq!(depth("Merge branch 'feature/INTEL-1'"), 0);
        assert_eq!(depth("fix: revert reference table for INTEL-3"), 0);
        assert_eq!(depth("Merge branch 'revert-4b89f7f6' into 'dev'"), 0);
        assert_eq!(depth("Revert \"Merge branch 'feature/INTEL-1'\""), 1);
        assert_eq!(
            depth("Revert \"Revert \"Merge branch 'feature/INTEL-1'\"\""),
            2
        );
        assert_eq!(depth("Reapply \"Merge branch 'feature/INTEL-1'\""), 2);
        assert_eq!(
            depth("Revert \"Reapply \"Merge branch 'feature/INTEL-1'\"\""),
            3,
            "a revert of a re-apply reverts again"
        );
        let original = commit(
            "aaaaaaa1",
            "Undo INTEL-1\n\nThis reverts commit bbbbbbb2.",
            &["p"],
        );
        let again = commit(
            "ccccccc3",
            "Back again\n\nThis reverts commit aaaaaaa1.",
            &["p"],
        );
        let known: HashMap<&str, &Commit> =
            [(original.hash.as_str(), &original)].into_iter().collect();
        assert_eq!(revert_depth(&original, &known), 1);
        assert_eq!(revert_depth(&again, &known), 2);
    }

    #[test]
    fn hotfix_bugfix_and_ticketless_branches_are_pseudo_tickets() {
        let mainline = [
            commit("r", "initial", &[]),
            merge(
                "h1",
                "Merge branch 'hotfix/fix-payment.timeout' into 'dev'",
                "r",
                "x1",
            ),
            merge("b1", "Merge branch 'BUGFIX/x_y' into 'dev'", "h1", "x2"),
            merge(
                "f1",
                "Merge branch 'hotfix/INTEL-5-thing' into 'dev'",
                "b1",
                "x3",
            ),
            merge("g1", "Merge branch 'fusion' into 'dev'", "f1", "x4"),
        ];
        let found: Vec<(String, TicketKind)> = scan_source(&mainline, &mainline, "INTEL", &names())
            .merged
            .into_iter()
            .map(|t| (t.ticket, t.kind))
            .collect();
        assert_eq!(
            found,
            [
                ("hotfix/fix-payment.timeout".into(), TicketKind::Hotfix),
                ("bugfix/x_y".into(), TicketKind::Bugfix),
                ("INTEL-5".into(), TicketKind::Jira),
                ("branch:fusion".into(), TicketKind::Branch),
            ]
        );
    }

    #[test]
    fn a_revert_on_dev_is_a_violation_and_cancels_the_merge() {
        let mainline = [
            commit("r", "initial", &[]),
            merge("m1", "Merge branch 'feature/INTEL-1' into 'dev'", "r", "f1"),
            merge(
                "m2",
                "Merge branch 'feature/INTEL-2' into 'dev'",
                "m1",
                "f2",
            ),
            commit(
                "v1",
                "Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"\n\nThis reverts commit m1, reversing\nchanges made to r.",
                &["m2"],
            ),
            merge(
                "m3",
                "Merge branch 'feature/INTEL-1' into 'dev'",
                "v1",
                "f3",
            ),
        ];
        let scan = scan_source(&mainline, &mainline, "INTEL", &names());
        assert_eq!(ids(&scan), [("INTEL-2", "m2"), ("INTEL-1", "m3")]);
        assert_eq!(scan.violations.len(), 1);
        assert_eq!(scan.violations[0].commit, "v1");
        assert_eq!(scan.violations[0].via, ViolationVia::Direct);
        assert!(!scan.violations[0].reapply);
        assert!(
            scan.uncoupled.is_empty(),
            "a revert is not an uncoupled change"
        );
    }

    #[test]
    fn a_reapply_on_dev_brings_the_ticket_back_and_is_a_violation() {
        let mainline = [
            commit("r", "initial", &[]),
            merge("m1", "Merge branch 'feature/INTEL-1' into 'dev'", "r", "f1"),
            commit(
                "v1",
                "Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"",
                &["m1"],
            ),
            commit(
                "v2",
                "Revert \"Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"\"",
                &["v1"],
            ),
        ];
        let scan = scan_source(&mainline, &mainline, "INTEL", &names());
        assert_eq!(ids(&scan), [("INTEL-1", "v2")]);
        assert_eq!(scan.violations.len(), 2);
        assert!(scan.violations[1].reapply);
    }

    /// GitLab's "Revert" button: a `revert-<sha>` branch holding the revert, merged into dev (f12479fe).
    #[test]
    fn a_gitlab_revert_merge_is_a_revert_not_a_second_merge() {
        let feature = commit("f1", "INTEL-24954 work", &["r"]);
        let revert = commit(
            "rv",
            "Revert \"Merge branch 'feature/INTEL-24954' into 'dev'\"\n\nThis reverts commit m1, reversing\nchanges made to r.",
            &["m1"],
        );
        let mainline = [
            commit("r", "initial", &[]),
            merge(
                "m1",
                "Merge branch 'feature/INTEL-24954' into 'dev'",
                "r",
                "f1",
            ),
            merge(
                "m2",
                "Merge branch 'revert-4b89f7f6' into 'dev'\n\nRevert \"Merge branch 'feature/INTEL-24954' into 'dev'\"\n\nSee merge request vdp/dbt-vdp!168",
                "m1",
                "rv",
            ),
        ];
        let all: Vec<Commit> = [
            mainline[0].clone(),
            feature,
            mainline[1].clone(),
            revert,
            mainline[2].clone(),
        ]
        .into();
        let scan = scan_source(&mainline, &all, "INTEL", &names());
        assert!(scan.merged.is_empty(), "{:?}", scan.merged);
        assert_eq!(scan.violations.len(), 1);
        assert_eq!(
            scan.violations[0].via,
            ViolationVia::RevertMerge { merge: "m2".into() }
        );
    }

    /// A release branch merged back into dev brings its reverts along (365e5322).
    #[test]
    fn a_back_merge_brings_reverts_and_is_a_violation() {
        let revert = commit(
            "rv",
            "Revert \"Merge branch 'feature/INTEL-2' into 'dev'\"",
            &["m2"],
        );
        let mainline = [
            commit("r", "initial", &[]),
            merge("m2", "Merge branch 'feature/INTEL-2' into 'dev'", "r", "f2"),
            merge("bm", "Merge branch 'release/2026_DROP04'", "m2", "rv"),
        ];
        let all: Vec<Commit> = [
            mainline[0].clone(),
            mainline[1].clone(),
            revert,
            mainline[2].clone(),
        ]
        .into();
        let scan = scan_source(&mainline, &all, "INTEL", &names());
        assert!(scan.merged.is_empty());
        assert_eq!(
            scan.violations[0].via,
            ViolationVia::BackMerge {
                merge: "bm".into(),
                from: "release/2026_DROP04".into()
            }
        );
    }

    #[test]
    fn a_sync_merge_of_dev_scans_what_it_brings() {
        let inner = merge("im", "Merge branch 'feature/INTEL-9' into 'dev'", "x", "f9");
        let mainline = [
            commit("r", "initial", &[]),
            merge("s", "Merge branch 'dev' from dbt-swf into dev", "r", "im"),
        ];
        let all: Vec<Commit> = [mainline[0].clone(), inner, mainline[1].clone()].into();
        let scan = scan_source(&mainline, &all, "INTEL", &names());
        assert_eq!(ids(&scan), [("INTEL-9", "im")]);
    }

    #[test]
    fn other_ids_in_a_merge_are_also_mentions() {
        let inner = commit("c1", "Feat / INTEL-27178 dubbele rijen", &["r"]);
        let mainline = [
            commit("r", "initial", &[]),
            merge(
                "m",
                "Merge branch 'feature/INTEL-27128' into 'dev'\n\nCloses INTEL-27128",
                "r",
                "c1",
            ),
        ];
        let all: Vec<Commit> = [mainline[0].clone(), inner, mainline[1].clone()].into();
        let scan = scan_source(&mainline, &all, "INTEL", &names());
        assert_eq!(ids(&scan), [("INTEL-27128", "m")]);
        assert_eq!(
            scan.also_mentions["INTEL-27128"].iter().collect::<Vec<_>>(),
            ["INTEL-27178"]
        );
    }

    #[test]
    fn uncoupled_commits_are_direct_changes() {
        let mainline = [
            commit("r", "initial", &[]),
            commit("d1", "fix typo in README", &["r"]),
            commit("d2", "INTEL-3 quick fix", &["d1"]),
            commit("d3", "fix: revert reference table", &["d2"]),
        ];
        let scan = scan_source(&mainline, &mainline, "INTEL", &names());
        assert_eq!(scan.uncoupled, [1, 3]);
        assert_eq!(ids(&scan), [("INTEL-3", "d2")]);
    }

    /// The patch tool re-enables with a revert of the revert; that ticket is not "reverted" any more (it was,
    /// and a second patch reverted the re-enable: the feature was removed again).
    #[test]
    fn compare_state_follows_reverts_and_re_enables() {
        let commits = [
            commit(
                "a",
                "Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"",
                &["p"],
            ),
            commit(
                "b",
                "Revert \"Merge branch 'feature/INTEL-2' into 'dev'\"",
                &["a"],
            ),
            commit(
                "c",
                "Revert \"Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"\"",
                &["b"],
            ),
            commit(
                "d",
                "feature/INTEL-27232: fix on the release branch",
                &["c"],
            ),
            commit(
                "e",
                "Merge branch 'feature/INTEL-5-revert-old-logic'",
                &["d", "x"],
            ),
            commit("f", "Revert INTEL-6 and INTEL-7", &["e"]),
        ];
        let state = compare_state(&commits, "INTEL");
        assert_eq!(
            state.reverted.keys().collect::<Vec<_>>(),
            ["INTEL-2", "INTEL-6", "INTEL-7"]
        );
        assert_eq!(state.reenabled["INTEL-1"], "c");
        assert!(state.present.contains("INTEL-27232"));
        assert!(
            state.present.contains("INTEL-5"),
            "a branch named revert-… is not a revert"
        );
    }
}
