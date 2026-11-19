//! The four things the release manager does with the framework repository, each previewed first (`dry_run`) and
//! then executed with a lease on exactly what the preview saw.
//!
//! - [`build_release`]: a `feature/<DROP>` branch from `main` with the chosen zip branches merged in and PML scripts.
//! - [`merge_into_environment`]: a release branch (or, where allowed, single zip branches) into an environment,
//!   with the merge message CI deploys on.
//! - [`commit_files`]: a direct fix on an environment, `main` (skeleton only) or a release branch.
//! - [`reset_branch`]: a branch back to an earlier commit (a force push, with the commits it drops listed).

mod build;
mod commit;
mod merge_env;
mod reset;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use vdp_git::{Applied, Operation, Vcs};
use vdp_release::{Conflicts, ResolutionAction, plain_report};

pub use build::{BuildOutcome, BuildRequest, PmlChange, build_release};
pub use commit::{CommitOutcome, CommitRequest, FileEdit, commit_files};
pub use merge_env::{LowerWarning, MergeOutcome, MergeRequest, merge_into_environment};
pub use reset::{ResetOutcome, ResetRequest, reset_branch};

use crate::zips::parse_zip_name;
use crate::{VsError, VsRules};

/// Shared collaborators and execution policy for the release flows.
#[derive(Clone, Copy)]
pub struct FlowContext<'a> {
    pub vcs: &'a dyn Vcs,
    pub rules: &'a VsRules,
    /// What the preview saw of the target branch on origin (`None`: absent), ignored when `control.dry_run`.
    pub expect: Option<&'a str>,
    pub control: &'a RunControl<'a>,
}

impl std::fmt::Debug for FlowContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowContext")
            .field("rules", &self.rules)
            .field("expect", &self.expect)
            .field("control", &self.control)
            .finish_non_exhaustive()
    }
}

pub(super) fn origin(branch: &str) -> String {
    format!("origin/{branch}")
}

/// Refuses when origin moved since the preview (`expect` = what the preview saw; `None` = absent).
pub(super) fn check_lease(
    vcs: &dyn Vcs,
    branch: &str,
    expect: Option<&str>,
) -> Result<Option<String>, VsError> {
    let now = vcs.remote_head(branch)?;
    if now.as_deref() != expect {
        let seen = |h: Option<&str>| {
            h.map_or_else(|| "absent".to_owned(), |h| h.chars().take(8).collect())
        };
        return Err(VsError::Stale(format!(
            "{branch} moved since the preview (preview: {}, origin now: {}): preview again",
            seen(expect),
            seen(now.as_deref())
        )));
    }
    Ok(now)
}

/// What happened to one source branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MergeStep {
    pub source: String,
    /// `merged`, `resolved` (after a conflict decision), `already` (nothing new), `skipped` (left out at a
    /// conflict) or `planned` (preview).
    pub outcome: String,
    pub message: String,
    pub files: Vec<String>,
}

/// How a run behaves: the preview flag, conflict handling and progress messages.
#[derive(Clone, Copy)]
pub struct RunControl<'a> {
    pub dry_run: bool,
    pub conflicts: Conflicts<'a>,
    pub progress: &'a dyn Fn(&str),
}

impl std::fmt::Debug for RunControl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunControl")
            .field("dry_run", &self.dry_run)
            .field("conflicts", &self.conflicts)
            .finish_non_exhaustive()
    }
}

/// Merges `source` (a remote-tracking ref) into the checked-out branch with `message`, pausing on conflicts.
pub(super) fn merge(
    vcs: &dyn Vcs,
    control: &RunControl<'_>,
    source: &str,
    rev: &str,
    target: &str,
) -> Result<MergeStep, VsError> {
    let message = VsRules::merge_message(source, target);
    let before = vcs.rev_parse("HEAD")?;
    if vcs.is_ancestor(rev, "HEAD")? {
        return Ok(MergeStep {
            source: source.to_owned(),
            outcome: "already".into(),
            message,
            files: Vec::new(),
        });
    }
    let mut applied = vcs.merge_with(rev, &message, control.conflicts.options)?;
    let mut outcome = "merged";
    loop {
        match applied {
            Applied::Conflicted(conflict) => {
                let Some(resolve) = control.conflicts.resolve else {
                    vcs.abort_op(Operation::Merge)?;
                    return Err(VsError::Aborted(format!(
                        "merging {source} into {target} conflicts in {}",
                        conflict
                            .paths
                            .iter()
                            .map(|p| p.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                };
                let report = plain_report(&format!("merge {source} into {target}"), &conflict);
                (control.progress)(&format!(
                    "conflict merging {source}: {} file(s); waiting for a decision",
                    report.files.len()
                ));
                let resolution = resolve(&report);
                match resolution.action {
                    ResolutionAction::Continue => {
                        for (path, choice) in &resolution.choices {
                            vcs.resolve_path(path, (*choice).into())?;
                        }
                        applied = vcs.continue_op(Operation::Merge, rev)?;
                        outcome = "resolved";
                    }
                    ResolutionAction::SkipStep => {
                        vcs.abort_op(Operation::Merge)?;
                        (control.progress)(&format!("{source} left out"));
                        return Ok(MergeStep {
                            source: source.to_owned(),
                            outcome: "skipped".into(),
                            message,
                            files: Vec::new(),
                        });
                    }
                    ResolutionAction::Abort => {
                        vcs.abort_op(Operation::Merge)?;
                        return Err(VsError::Aborted(format!(
                            "merge of {source} stopped; nothing was pushed"
                        )));
                    }
                }
            }
            Applied::Empty => {
                outcome = "already";
                break;
            }
            Applied::Committed | Applied::Resolved(_) => break,
        }
    }
    let files = changed_files(vcs, &before, "HEAD")?;
    (control.progress)(&format!("merged {source} ({} file(s))", files.len()));
    Ok(MergeStep {
        source: source.to_owned(),
        outcome: outcome.into(),
        message,
        files,
    })
}

pub(super) fn changed_files(vcs: &dyn Vcs, from: &str, to: &str) -> Result<Vec<String>, VsError> {
    let mut files = BTreeSet::new();
    for entry in vcs.path_log(from, to, ".")? {
        files.extend(entry.files.into_iter().map(|f| f.path));
    }
    Ok(files.into_iter().collect())
}

/// The zips and PML scripts staged at `rev`.
pub(super) fn staged(
    vcs: &dyn Vcs,
    rules: &VsRules,
    rev: &str,
) -> Result<(Vec<String>, Vec<String>), VsError> {
    let s = &rules.settings;
    let zips = vcs
        .files_at(rev, &s.staging_dir)?
        .iter()
        .filter_map(|p| parse_zip_name(p).map(|(n, _, _)| n))
        .collect();
    let pml = vcs
        .files_at(rev, &s.pml_dir)?
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    Ok((zips, pml))
}
