//! Commit-graph traversal and ticket-state aggregation over source and compare branches.

use std::collections::{BTreeSet, HashMap, HashSet};

use vdp_git::Commit;

use super::parser::{
    branch_change, merge_branch_name, revert_depth, revert_target, strip_reverts, ticket_ids,
    ticket_kind, ticket_of,
};
use super::{
    CompareState, MergedTicket, RawViolation, ScanNames, SourceScan, TicketKind, ViolationVia,
};

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

/// Inputs for [`scan_source`].
#[derive(Debug, Clone, Copy)]
pub struct SourceScanInput<'a> {
    pub mainline: &'a [Commit],
    pub all: &'a [Commit],
    pub prefix: &'a str,
    pub names: &'a ScanNames,
}

/// Scans the source: `mainline` is `log --first-parent merge_base..source`, `all` is `log merge_base..source`
/// (both oldest first).
pub fn scan_source(input: SourceScanInput<'_>) -> SourceScan {
    let mut scanner = Scanner {
        prefix: input.prefix,
        names: input.names,
        known: input
            .all
            .iter()
            .chain(input.mainline)
            .map(|c| (c.hash.as_str(), c))
            .collect(),
        order: input
            .all
            .iter()
            .enumerate()
            .map(|(i, c)| (c.hash.as_str(), i))
            .collect(),
        mainline: input.mainline.iter().map(|c| c.hash.as_str()).collect(),
        out: SourceScan::default(),
    };
    for (position, commit) in input.mainline.iter().enumerate() {
        scanner.visit(commit, position, false);
    }
    scanner.out
}

/// The tickets merged by `commits` (a mainline, oldest first), without the merge contents: see [`scan_source`].
pub fn merged_tickets(commits: &[Commit], prefix: &str) -> Vec<MergedTicket> {
    scan_source(SourceScanInput {
        mainline: commits,
        all: commits,
        prefix,
        names: &ScanNames::default(),
    })
    .merged
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
