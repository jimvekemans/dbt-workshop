//! Promotion of a release branch to main (`build_new_release`, `build_patch_release`,
//! `check_deploy_type_for_main`, `deploy_existing_release`).

use serde::{Deserialize, Serialize};
use vdp_git::{Applied, ApplyOptions, Choice, OnConflict, Operation, PushMode, Strategy, Vcs};
use vdp_jira::FixVersion;

use crate::analysis::CommitInfo;
use crate::conflicts::{Conflicts, ResolutionAction, plain_report};
use crate::scan::merge_branch_name;
use crate::{BranchRules, ReleaseError, ReleaseOnly, release_only, remote_ref};

/// How a release reaches main.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PromotionKind {
    /// A new release: main is reset to the merge-base of the source and main, the release is merged with
    /// `--no-ff`, and main is pushed with a lease.
    New,
    /// A patch of the release main already has: merged with `--no-ff` on top of main.
    Patch,
}

/// What [`detect`] found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Detection {
    pub kind: PromotionKind,
    pub main_head: CommitInfo,
    /// Why it is this kind.
    pub reason: String,
    /// The branch is entirely on main already.
    pub nothing_to_promote: bool,
    /// A newer release main already has: promoting this one rolls main back to an older release.
    pub rollback_from: Option<String>,
}

/// By ancestry, not by main's last commit message:
/// - the release head is in main → nothing to promote;
/// - main shares a commit with the release that the source does not have (its reverts, its changelog) → a
///   patch of the release main has;
/// - otherwise a new release.
pub fn detect(vcs: &dyn Vcs, branch: &str, rules: &BranchRules) -> Result<Detection, ReleaseError> {
    let branch = rules.release_target(branch)?;
    let release = remote_ref(&branch);
    let main = remote_ref(&rules.main);
    let source = remote_ref(&rules.source);
    let release_head = vcs.rev_parse(&release)?;
    let head = vcs.commit(&main)?;
    let shared = vcs.merge_base(&main, &release)?;
    let (kind, nothing, reason) = if vcs.is_ancestor(&release_head, &main)? {
        (
            PromotionKind::Patch,
            true,
            format!("{branch} is already on {}", rules.main),
        )
    } else if !vcs.is_ancestor(&shared, &source)? {
        (
            PromotionKind::Patch,
            false,
            format!(
                "{} already has {} of {branch} (a commit {} does not have): this is a patch",
                rules.main,
                &shared[..shared.len().min(8)],
                rules.source
            ),
        )
    } else {
        (
            PromotionKind::New,
            false,
            format!("{} has nothing of {branch} yet: a new release", rules.main),
        )
    };
    Ok(Detection {
        kind,
        main_head: CommitInfo::from_commit(&head, None),
        reason,
        nothing_to_promote: nothing,
        rollback_from: newer_release_on_main(vcs, &branch, rules)?,
    })
}

/// The newest release merged into main (along main's mainline since it split from the source) when it is
/// newer than `branch`.
fn newer_release_on_main(
    vcs: &dyn Vcs,
    branch: &str,
    rules: &BranchRules,
) -> Result<Option<String>, ReleaseError> {
    let version = |name: &str| -> Option<FixVersion> {
        name.strip_prefix(&rules.prefix)
            .and_then(|v| v.parse().ok())
    };
    let Some(mine) = version(branch) else {
        return Ok(None);
    };
    let main = remote_ref(&rules.main);
    let base = vcs.merge_base(&remote_ref(&rules.source), &main)?;
    let newest = vcs
        .log(&base, &main, true)?
        .iter()
        .filter(|c| c.is_merge())
        .filter_map(|c| merge_branch_name(&c.message))
        .filter_map(|name| version(&name).map(|v| (v, name)))
        .max_by(|(a, _), (b, _)| a.cmp(b));
    Ok(newest.filter(|(v, _)| *v > mine).map(|(_, name)| name))
}

/// How to promote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PromoteOptions {
    /// `None`: as detected.
    pub kind: Option<PromotionKind>,
    pub dry_run: bool,
    /// main as the preview saw it: refused if it moved since, and the push leases on it.
    pub expect_main: Option<String>,
    /// Promoting an older release than the one main has was shown and accepted.
    pub accept_rollback: bool,
    /// The commits a new promotion drops from main were shown and accepted.
    pub accept_losses: bool,
}

/// The result of a promotion.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Promoted {
    pub branch: String,
    pub kind: Option<PromotionKind>,
    pub dry_run: bool,
    pub reason: String,
    pub nothing_to_promote: bool,
    pub rollback_from: Option<String>,
    /// main before (pass it back to execute exactly what was previewed).
    pub main_before: String,
    /// For a new release: what main is reset to.
    pub merge_base: Option<String>,
    /// For a new release: commits main has that neither the release nor the source has; the reset drops them.
    pub losses: Vec<ReleaseOnly>,
    pub main_head: Option<String>,
    pub pushed: bool,
}

/// Promotes release branches to main.
pub struct Promotion<'a> {
    pub vcs: &'a dyn Vcs,
    /// main, the source (for a new release's merge-base) and which branches are releases.
    pub rules: &'a BranchRules,
    pub progress: &'a dyn Fn(&str),
    /// A merge conflict outside `changelog/` is put to `resolve`; without one the promotion stops.
    pub conflicts: Conflicts<'a>,
}

impl std::fmt::Debug for Promotion<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Promotion")
            .field("rules", &self.rules)
            .finish_non_exhaustive()
    }
}

impl Promotion<'_> {
    /// `merge --no-ff` of the release. A conflict only in `changelog/` (a patch rewrote the release's
    /// changelog that main has) takes the release's version; any other conflict is put to the release manager.
    fn merge(&self, release: &str, message: &str) -> Result<(), ReleaseError> {
        let options = ApplyOptions {
            on_conflict: OnConflict::Pause,
            strategy: Strategy::Strict,
        };
        let mut applied = self.vcs.merge_with(release, message, options)?;
        loop {
            let Applied::Conflicted(conflict) = applied else {
                return Ok(());
            };
            let changelog_only = conflict
                .paths
                .iter()
                .all(|p| p.path.starts_with("changelog/"));
            if changelog_only {
                for path in &conflict.paths {
                    self.vcs.resolve_path(&path.path, Choice::Change)?;
                }
                (self.progress)("changelog conflict: the release's changelog wins");
                applied = self.vcs.continue_op(Operation::Merge, release)?;
                continue;
            }
            let Some(resolve) = self.conflicts.resolve else {
                self.vcs.abort_op(Operation::Merge)?;
                return Err(ReleaseError::Apply {
                    action: format!("merging {release}"),
                    detail: format!(
                        "conflicts in {}",
                        conflict
                            .paths
                            .iter()
                            .map(|p| p.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            };
            let report = plain_report(
                &format!("merge {release} into {}", self.rules.main),
                &conflict,
            );
            (self.progress)(&format!(
                "merge conflict in {} file(s); waiting for a decision",
                report.files.len()
            ));
            let resolution = resolve(&report);
            match resolution.action {
                ResolutionAction::Continue => {
                    for (path, choice) in &resolution.choices {
                        self.vcs.resolve_path(path, (*choice).into())?;
                    }
                    applied = self.vcs.continue_op(Operation::Merge, release)?;
                }
                ResolutionAction::SkipStep | ResolutionAction::Abort => {
                    self.vcs.abort_op(Operation::Merge)?;
                    return Err(ReleaseError::Aborted(format!(
                        "merge of {release} stopped; nothing was pushed"
                    )));
                }
            }
        }
    }

    /// Fixes made directly on main (or a previous release) that neither the source nor the release has: a new
    /// promotion's reset drops them.
    fn losses(&self, release: &str, main: &str) -> Result<Vec<ReleaseOnly>, ReleaseError> {
        let in_release: std::collections::BTreeSet<String> = self
            .vcs
            .cherry(release, main)?
            .into_iter()
            .filter(|(_, equivalent)| *equivalent)
            .map(|(hash, _)| hash)
            .collect();
        Ok(release_only(
            self.vcs,
            &self.rules.source,
            main,
            &self.rules.ticket_prefix,
        )?
        .into_iter()
        .filter(|lost| !in_release.contains(&lost.commit.hash))
        .filter(|lost| {
            !self
                .vcs
                .is_ancestor(&lost.commit.hash, release)
                .unwrap_or(false)
        })
        .collect())
    }

    /// A rollback and dropped commits need explicit acceptance.
    fn check_accepted(
        &self,
        result: &Promoted,
        options: &PromoteOptions,
    ) -> Result<(), ReleaseError> {
        let (main, branch) = (&self.rules.main, &result.branch);
        if let Some(newer) = &result.rollback_from
            && !options.accept_rollback
        {
            return Err(ReleaseError::Refused(format!(
                "{main} has {newer}; promoting the older {branch} rolls it back: accept that explicitly"
            )));
        }
        if !result.losses.is_empty() && !options.accept_losses {
            return Err(ReleaseError::Refused(format!(
                "the new promotion drops {} commit(s) from {main} that {} does not have: bring them to {} or accept the loss",
                result.losses.len(),
                self.rules.source,
                self.rules.source
            )));
        }
        Ok(())
    }

    pub fn run(&self, branch: &str, options: &PromoteOptions) -> Result<Promoted, ReleaseError> {
        if let Err(error) = self.vcs.fetch_all() {
            (self.progress)(&format!(
                "fetch failed (continuing with local refs): {error}"
            ));
        }
        let branch = self.rules.release_target(branch)?;
        let release = remote_ref(&branch);
        let main = self.rules.main.clone();
        let main_remote = remote_ref(&main);
        self.vcs
            .rev_parse(&release)
            .map_err(|_| ReleaseError::Invalid(format!("{release} does not exist")))?;
        let found = detect(self.vcs, &branch, self.rules)?;
        (self.progress)(&format!(
            "{main} is at {} ({}): {}",
            found.main_head.short, found.main_head.subject, found.reason
        ));
        let kind = options.kind.unwrap_or(found.kind);
        let main_before = self.vcs.rev_parse(&main_remote)?;
        let mut result = Promoted {
            branch: branch.clone(),
            kind: Some(kind),
            dry_run: options.dry_run,
            reason: found.reason,
            nothing_to_promote: found.nothing_to_promote,
            rollback_from: found.rollback_from,
            main_before: main_before.clone(),
            ..Promoted::default()
        };
        if let Some(expected) = &options.expect_main
            && *expected != main_before
        {
            return Err(ReleaseError::Stale(format!(
                "{main} moved since the preview; preview again"
            )));
        }
        if result.nothing_to_promote && kind == PromotionKind::Patch {
            (self.progress)(&format!(
                "nothing to promote: {branch} is already on {main}"
            ));
            return Ok(result);
        }
        if let Some(newer) = &result.rollback_from {
            (self.progress)(&format!(
                "{main} has {newer}, newer than {branch}: promoting it rolls {main} back"
            ));
        }
        if kind == PromotionKind::New {
            result.merge_base = Some(
                self.vcs
                    .merge_base(&remote_ref(&self.rules.source), &main_remote)?,
            );
            result.losses = self.losses(&release, &main_remote)?;
            for lost in &result.losses {
                (self.progress)(&format!(
                    "the reset drops {} {} (not on {} nor in {branch})",
                    lost.commit.short, lost.commit.subject, self.rules.source
                ));
            }
        }
        if options.dry_run {
            if let Some(base) = &result.merge_base {
                (self.progress)(&format!(
                    "[dry run] would reset {main} to {}",
                    &base[..base.len().min(12)]
                ));
            }
            (self.progress)(&format!(
                "[dry run] would merge {branch} into {main} (--no-ff) and push"
            ));
            return Ok(result);
        }
        self.check_accepted(&result, options)?;
        if self.vcs.is_dirty()? {
            return Err(ReleaseError::Refused(
                "the work tree has uncommitted changes; commit or stash them first".into(),
            ));
        }
        self.vcs.checkout_reset(&main, &main_remote)?;
        if let Some(base) = &result.merge_base {
            (self.progress)(&format!(
                "resetting {main} to the merge-base {}",
                &base[..base.len().min(12)]
            ));
            self.vcs.reset_hard(base)?;
        }
        (self.progress)(&format!("merging {branch} into {main}"));
        let message = format!("Merge branch '{branch}' into {main}");
        self.merge(&release, &message)?;
        (self.progress)(&format!("pushing {main} (leased on {})", &main_before[..8]));
        self.vcs
            .push(&main, PushMode::Expect(Some(main_before.clone())))?;
        result.main_head = Some(self.vcs.rev_parse(&main)?);
        result.pushed = true;
        Ok(result)
    }
}
