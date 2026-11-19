//! Direct-commit flow on environments, release branches, or `main` skeleton paths.

use serde::{Deserialize, Serialize};
use vdp_git::{FileChange, PushMode};

use super::{FlowContext, changed_files, check_lease, origin};
use crate::{VsError, VsRules};

/// One file to write or delete in a direct fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileEdit {
    pub path: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub content: Option<String>,
}

/// A direct fix: the files to change on a branch and the commit message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommitRequest {
    pub branch: String,
    pub edits: Vec<FileEdit>,
    pub message: String,
}

/// What a direct fix did (or, in a dry run, would do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommitOutcome {
    pub branch: String,
    pub origin_head: String,
    pub commit: Option<String>,
    pub files: Vec<String>,
    pub pushed: bool,
}

/// A direct fix: environments and release branches anything, `main` only skeleton paths. The message may not
/// look like a merge (CI would deploy on it).
///
/// # Errors
///
/// [`VsError::Stale`] when origin's branch moved since the preview (`ctx.expect`; not checked in a dry run);
/// [`VsError::Refused`] when the branch is not an environment, a release branch or `main`, or `main` would get a path
/// that belongs to the environments; [`VsError::Invalid`] when the message is empty or looks like a merge, there are no
/// files, or the branch does not exist on origin; [`VsError::Git`] when a git operation fails (including a rejected push).
pub fn commit_files(
    ctx: &FlowContext<'_>,
    request: &CommitRequest,
) -> Result<CommitOutcome, VsError> {
    let FlowContext {
        vcs,
        rules,
        expect,
        control,
    } = *ctx;
    vcs.fetch_all()?;
    let branch = request.branch.as_str();
    let is_main = branch == rules.settings.main;
    if !(is_main || rules.is_environment(branch) || rules.is_release_branch(branch)) {
        return Err(VsError::Refused(format!(
            "{branch} is not changed by hand here (environments, release branches and {} only)",
            rules.settings.main
        )));
    }
    let message = request.message.trim();
    if message.is_empty() {
        return Err(VsError::Invalid("a commit message is required".into()));
    }
    if VsRules::triggers_deploy(message) {
        return Err(VsError::Invalid(
            "the message may not start with `Merge branch` or `Auto-merge`: CI would deploy on it"
                .into(),
        ));
    }
    if request.edits.is_empty() {
        return Err(VsError::Invalid("no files to change".into()));
    }
    if is_main {
        rules.check_skeleton(request.edits.iter().map(|e| e.path.as_str()))?;
    }
    let origin_head = if control.dry_run {
        vcs.remote_head(branch)?
    } else {
        check_lease(vcs, branch, expect)?
    }
    .ok_or_else(|| VsError::Invalid(format!("{branch} does not exist on origin")))?;
    vcs.checkout_reset(branch, &origin(branch))?;
    let changes: Vec<FileChange> = request
        .edits
        .iter()
        .map(|e| FileChange {
            path: e.path.trim().to_owned(),
            content: e.content.clone().map(String::into_bytes),
        })
        .collect();
    let commit = vcs.commit_changes(&changes, message)?;
    let files = match &commit {
        Some(c) => changed_files(vcs, &origin_head, c)?,
        None => Vec::new(),
    };
    let mut outcome = CommitOutcome {
        branch: branch.to_owned(),
        origin_head: origin_head.clone(),
        commit: commit.clone(),
        files,
        pushed: false,
    };
    if control.dry_run || commit.is_none() {
        return Ok(outcome);
    }
    vcs.push(branch, PushMode::Expect(Some(origin_head)))?;
    (control.progress)(&format!("pushed {branch}"));
    outcome.pushed = true;
    Ok(outcome)
}
