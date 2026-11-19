//! What origin has that a flow would lose, and which release branches the flows may touch.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use vdp_git::{Commit, Vcs};
use vdp_jira::FixVersion;

use crate::analysis::CommitInfo;
use crate::scan::{compare_state, revert_depth, ticket_ids};
use crate::{BranchRules, ReleaseError, release_branches, remote_ref};

/// The trailer of the release changelog commit.
pub const CHANGELOG_TRAILER: &str = "Vdp-Changelog:";

/// A branch on origin as a preview saw it: executing refuses if it moved since (and pushes with a lease on
/// exactly this commit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct OriginState {
    pub branch: String,
    /// `None`: origin does not have the branch.
    pub head: Option<String>,
}

/// A commit that only a release branch (or main) has: a fix made there directly. The developers bring such
/// fixes to dev themselves; the release tools never write dev.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ReleaseOnly {
    pub commit: CommitInfo,
    pub tickets: Vec<String>,
}

/// Inputs for [`release_only`].
#[derive(Debug, Clone, Copy)]
pub struct ReleaseOnlyRequest<'a> {
    pub source: &'a str,
    pub head: &'a str,
    pub ticket_prefix: &'a str,
}

/// Inputs for [`check_scope`].
#[derive(Debug, Clone, Copy)]
pub struct ScopeCheck<'a> {
    pub rules: &'a BranchRules,
    pub branch: &'a str,
    pub action: ScopeAction,
}

/// The commits of `head` whose change `source` lacks: not merges, reverts or re-enables, not the changelog,
/// no equivalent change (patch-id) on `source`, and no ticket of theirs merged on `source` since they split
/// (the developers' own MR to dev carries the same ticket under another patch-id).
///
/// # Errors
///
/// [`ReleaseError::Git`] when the history cannot be read.
pub fn release_only(
    vcs: &dyn Vcs,
    request: &ReleaseOnlyRequest<'_>,
) -> Result<Vec<ReleaseOnly>, ReleaseError> {
    let source = remote_ref(request.source);
    let head = remote_ref(request.head);
    let ticket_prefix = request.ticket_prefix;
    let base = vcs.merge_base(&source, &head)?;
    let on_source = compare_state(&vcs.log(&base, &source, false)?, ticket_prefix).present;
    let commits = vcs.log(&base, &head, false)?;
    let known: HashMap<&str, &Commit> = commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    let missing: BTreeSet<String> = vcs
        .cherry(&source, &head)?
        .into_iter()
        .filter(|(_, equivalent)| !equivalent)
        .map(|(hash, _)| hash)
        .collect();
    Ok(commits
        .iter()
        .filter(|c| missing.contains(&c.hash))
        .filter(|c| !c.is_merge() && revert_depth(c, &known) == 0)
        .filter(|c| !c.message.contains(CHANGELOG_TRAILER))
        .filter_map(|c| {
            let tickets = ticket_ids(&c.message, ticket_prefix);
            let on_dev = !tickets.is_empty() && tickets.iter().all(|t| on_source.contains(t));
            (!on_dev).then(|| ReleaseOnly {
                commit: CommitInfo::from_commit(c, None),
                tickets,
            })
        })
        .collect())
}

/// Which release branches the flows may change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum BranchRole {
    /// The newest release branch (by fix version).
    Newest,
    /// The one before it.
    Previous,
    /// Older (or not a DROP name): read-only for the release tools.
    ReadOnly,
}

/// What an action does to a release branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeAction {
    /// Cut it (new) or recreate it (`recreate`).
    Release {
        recreate: bool,
    },
    Patch,
    Promote,
}

/// Refuses actions on release branches outside the newest two (and recreating a promoted one, or cutting a
/// release older than the newest).
///
/// # Errors
///
/// [`ReleaseError::Invalid`] when `branch` is not a release branch, the action is on a release outside the newest two,
/// recreates a promoted release or cuts one older than the newest; [`ReleaseError::Git`] when the branches cannot be
/// listed.
pub fn check_scope(vcs: &dyn Vcs, check: &ScopeCheck<'_>) -> Result<(), ReleaseError> {
    let ScopeCheck {
        rules,
        branch,
        action,
    } = *check;
    let branch = rules.release_target(branch)?;
    let branches = release_branches(vcs, &rules.prefix, &rules.main)?;
    let existing = branches.iter().find(|b| b.name == branch);
    match (action, existing) {
        (ScopeAction::Release { .. }, None) => {
            let version: Option<FixVersion> = branch
                .strip_prefix(&rules.prefix)
                .and_then(|s| s.parse().ok());
            let newest = branches.iter().find(|b| b.role == BranchRole::Newest);
            let newest_version: Option<FixVersion> = newest
                .and_then(|b| b.fix_version.as_deref())
                .and_then(|v| v.parse().ok());
            match (version, newest_version, newest) {
                (Some(new), Some(top), Some(top_branch)) if new <= top => {
                    Err(ReleaseError::Invalid(format!(
                        "{branch} is older than {}: a new release must be the newest",
                        top_branch.name
                    )))
                }
                _ => Ok(()),
            }
        }
        (_, None) => Err(ReleaseError::Invalid(format!(
            "{branch} does not exist on origin"
        ))),
        (_, Some(found)) if found.role == BranchRole::ReadOnly => {
            Err(ReleaseError::Invalid(format!(
                "{branch} is read-only for the release tools: only the newest release branch and the one before it may be changed"
            )))
        }
        (ScopeAction::Release { recreate: true }, Some(found)) if found.promoted => {
            Err(ReleaseError::Invalid(format!(
                "{branch} is already on main: patch it instead of recreating it"
            )))
        }
        (ScopeAction::Release { recreate: false }, Some(_)) => Err(ReleaseError::Invalid(format!(
            "{branch} exists on origin: recreate it, or patch it"
        ))),
        _ => Ok(()),
    }
}

/// `origin/<branch>` now.
///
/// # Errors
///
/// [`ReleaseError::Git`] when the remote head cannot be read.
pub fn origin_state(vcs: &dyn Vcs, branch: &str) -> Result<OriginState, ReleaseError> {
    Ok(OriginState {
        branch: branch.to_owned(),
        head: vcs.remote_head(branch)?,
    })
}
