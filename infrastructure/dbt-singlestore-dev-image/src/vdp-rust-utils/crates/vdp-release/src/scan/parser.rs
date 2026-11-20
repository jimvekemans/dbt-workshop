//! Commit subject, branch name, and revert-chain parsers for release scanning.

use std::collections::HashMap;

use vdp_git::Commit;

use super::TicketKind;

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

/// The first `hotfix/<slug>` or `bugfix/<slug>` in a message (as the Python release service matched it).
pub(super) fn branch_change(message: &str) -> Option<(TicketKind, String)> {
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
pub(super) fn revert_target(message: &str, prefix: &str) -> Option<String> {
    ticket_ids(message, prefix)
        .into_iter()
        .next()
        .or_else(|| branch_change(message).map(|(_, id)| id))
        .or_else(|| merge_branch_name(&strip_reverts(message)).map(|b| format!("branch:{b}")))
}

pub(super) fn strip_reverts(message: &str) -> String {
    let subject = message.lines().next().unwrap_or_default();
    quoted_depth(subject).1.trim_end_matches('"').to_owned()
}

/// The ticket a (non-revert) commit belongs to: its first ticket id, else its hotfix/bugfix branch.
pub(super) fn ticket_of(message: &str, prefix: &str) -> Option<(TicketKind, String)> {
    ticket_ids(message, prefix)
        .into_iter()
        .next()
        .map(|id| (TicketKind::Jira, id))
        .or_else(|| branch_change(message))
}

pub(super) fn ticket_kind(id: &str) -> TicketKind {
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
