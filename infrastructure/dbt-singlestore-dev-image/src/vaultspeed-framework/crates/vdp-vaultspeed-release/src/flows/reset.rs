//! Branch reset flow moving a writable branch back to an earlier ancestor commit.

use serde::{Deserialize, Serialize};
use vdp_git::PushMode;

use super::{FlowContext, check_lease};
use crate::VsError;
use crate::status::CommitRef;

/// Which branch to reset and the commit it goes back to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ResetRequest {
    pub branch: String,
    /// The commit the branch goes back to (an ancestor of its head).
    pub target: String,
    #[serde(default)]
    pub accept_losses: bool,
}

/// What resetting a branch did (or, in a dry run, would do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ResetOutcome {
    pub branch: String,
    pub origin_head: String,
    pub target: CommitRef,
    /// The commits the reset drops, oldest first.
    pub losses: Vec<CommitRef>,
    pub pushed: bool,
}

/// A branch back to an earlier commit of its own (a force push with a lease). Zip branches are never touched
/// (the guard), and the target must be in the branch's history.
///
/// # Errors
///
/// [`VsError::Stale`] when origin's branch moved since the preview (`ctx.expect`; not checked in a dry run);
/// [`VsError::Refused`] when the reset drops commits that were not accepted;
/// [`VsError::Invalid`] when the branch does not exist on origin, the target is not a commit, or it is not an earlier
/// commit of the branch; [`VsError::Git`] when the branch is protected (a zip branch: `GitError::Guard`) or a git
/// operation fails (including a rejected push).
pub fn reset_branch(
    ctx: &FlowContext<'_>,
    request: &ResetRequest,
) -> Result<ResetOutcome, VsError> {
    let FlowContext {
        vcs,
        rules,
        expect,
        control,
    } = *ctx;
    vcs.fetch_all()?;
    let branch = request.branch.as_str();
    rules.guard().check(branch)?;
    let origin_head = if control.dry_run {
        vcs.remote_head(branch)?
    } else {
        check_lease(vcs, branch, expect)?
    }
    .ok_or_else(|| VsError::Invalid(format!("{branch} does not exist on origin")))?;
    let target = vcs
        .rev_parse(request.target.trim())
        .map_err(|_| VsError::Invalid(format!("{} is not a commit", request.target)))?;
    if target == origin_head || !vcs.is_ancestor(&target, &origin_head)? {
        return Err(VsError::Invalid(format!(
            "{} is not an earlier commit of {branch}",
            &target[..target.len().min(8)]
        )));
    }
    let losses: Vec<CommitRef> = vcs
        .log(&target, &origin_head, false)?
        .iter()
        .map(CommitRef::from)
        .collect();
    let mut outcome = ResetOutcome {
        branch: branch.to_owned(),
        origin_head: origin_head.clone(),
        target: CommitRef::from(&vcs.commit(&target)?),
        losses,
        pushed: false,
    };
    if control.dry_run {
        return Ok(outcome);
    }
    if !request.accept_losses {
        return Err(VsError::Refused(format!(
            "resetting {branch} drops {} commit(s); tick to accept",
            outcome.losses.len()
        )));
    }
    vcs.checkout_reset(branch, &target)?;
    vcs.push(branch, PushMode::Expect(Some(origin_head)))?;
    (control.progress)(&format!("reset {branch} to {}", outcome.target.short));
    outcome.pushed = true;
    Ok(outcome)
}
