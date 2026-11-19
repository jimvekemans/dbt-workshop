//! A patch: tickets added to an existing release branch (`run_patch_release`).

use std::collections::BTreeSet;

use vdp_git::{PushMode, Vcs};

use crate::analysis::{Analysis, AnalysisKind};
use crate::changelog::{self, Changelog};
use crate::flow::{
    ChangelogFile, Outcome, Step, StepOutcome, StepRunner, check_fresh, commit_changelog, releaser,
};
use crate::{BranchRules, OriginState, ReleaseError};

/// The git actions that add `include` to the release branch, oldest change first: tickets that came with the
/// release and were reverted on it (merged before the cut), then tickets merged since, in mainline order:
/// - a ticket the release branch reverted gets that revert reverted (it is back in);
/// - its merges on the source since the release was cut are cherry-picked, merges against their mainline
///   parent (`-m 1`), so the whole feature comes along. The Python version picked only the feature's last
///   commit.
pub fn patch_steps(analysis: &Analysis, include: &BTreeSet<String>) -> Vec<Step> {
    let mut tickets: Vec<_> = analysis
        .tickets
        .iter()
        .filter(|t| include.contains(&t.id))
        .collect();
    tickets.sort_by_key(|t| {
        t.merges
            .first()
            .and_then(|m| m.position)
            .map_or(0, |p| u64::from(p) + 1)
    });
    let mut steps = Vec::new();
    for ticket in tickets {
        if let Some(revert) = &ticket.reverted_on_compare {
            steps.push(Step::ReEnable {
                ticket: ticket.id.clone(),
                commit: revert.hash.clone(),
                subject: revert.subject.clone(),
            });
        }
        for merge in &ticket.merges {
            steps.push(Step::Pick {
                ticket: ticket.id.clone(),
                commit: merge.hash.clone(),
                subject: merge.subject.clone(),
            });
        }
    }
    steps
}

/// The patch flow.
pub struct PatchFlow<'a> {
    pub vcs: &'a dyn Vcs,
    /// The patched branch must be a release branch (never the source).
    pub rules: &'a BranchRules,
    pub confirm: &'a dyn Fn(&Step, usize, usize) -> bool,
    pub progress: &'a dyn Fn(&str),
    /// Conflict handling (default: resolve for the change being applied, as before).
    pub conflicts: crate::conflicts::Conflicts<'a>,
    /// Rewrite the release changelog: drop its commit, apply the steps, commit the new one on top.
    pub changelog: bool,
}

impl std::fmt::Debug for PatchFlow<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatchFlow")
            .field("vcs", &self.vcs)
            .finish_non_exhaustive()
    }
}

/// Options for [`PatchFlow::run`].
#[derive(Debug, Clone, Copy)]
pub struct PatchOptions {
    pub dry_run: bool,
    pub interactive: bool,
}

impl PatchFlow<'_> {
    /// The changelog stays the branch's last commit: the current one is taken out (dropped at the tip, or
    /// rebased away), and written anew after the steps.
    fn take_out_changelog(&self, analysis: &Analysis) -> Result<(), ReleaseError> {
        let trailer = format!(
            "{} {}",
            crate::CHANGELOG_TRAILER,
            analysis.request.fix_version
        );
        let current = self
            .vcs
            .log(&analysis.merge_base, &analysis.compare_head, false)?
            .into_iter()
            .rfind(|c| c.message.contains(&trailer));
        if let Some(commit) = current {
            if commit.hash == analysis.compare_head {
                self.vcs.reset_hard("HEAD~1")?;
            } else {
                self.vcs
                    .rebase_onto(&format!("{}^", commit.hash), &commit.hash)?;
            }
            (self.progress)(&format!(
                "changelog commit {} taken out; it is rewritten after the patch",
                commit.short()
            ));
        }
        Ok(())
    }

    /// The changelog a patch would write (dry run).
    fn planned_changelog(
        &self,
        analysis: &Analysis,
        steps: &[Step],
        previous: Option<Changelog>,
    ) -> ChangelogFile {
        let planned: Vec<(Step, StepOutcome)> = steps
            .iter()
            .map(|s| (s.clone(), StepOutcome::Planned))
            .collect();
        let branch = crate::local_name(&analysis.request.compare);
        let log = changelog::for_patch(
            previous,
            analysis,
            &branch,
            &planned,
            &releaser(self.vcs),
            &changelog::today(),
        );
        ChangelogFile {
            path: changelog::path(&log.release),
            content: log.render(),
        }
    }

    /// Checks out the release branch as it is on origin, applies the steps and pushes (no force).
    ///
    /// # Errors
    ///
    /// [`ReleaseError::Invalid`] when `analysis` is not a patch analysis, the branch is not a release branch or the selection
    /// is invalid; [`ReleaseError::Stale`] when a ref moved since the analysis; [`ReleaseError::Refused`] when the work tree
    /// is dirty; [`ReleaseError::Apply`] when a step fails; [`ReleaseError::Aborted`] when the release manager stops at a
    /// conflict; [`ReleaseError::Git`] when another git operation fails (including a rejected push).
    pub fn run(
        &self,
        analysis: &Analysis,
        include: &BTreeSet<String>,
        options: &PatchOptions,
    ) -> Result<Outcome, ReleaseError> {
        let PatchOptions {
            dry_run,
            interactive,
        } = *options;
        if analysis.request.kind != AnalysisKind::Patch {
            return Err(ReleaseError::Invalid("this is not a patch analysis".into()));
        }
        let branch = self.rules.release_target(&analysis.request.compare)?;
        analysis.validate_selection(include)?;
        let steps = patch_steps(analysis, include);
        let mut outcome = Outcome {
            branch: branch.clone(),
            dry_run,
            origin: Some(OriginState {
                branch: branch.clone(),
                head: Some(analysis.compare_head.clone()),
            }),
            ..Outcome::default()
        };
        check_fresh(self.vcs, analysis, self.progress)?;
        let path = changelog::path(&analysis.request.fix_version);
        let previous = if self.changelog {
            self.vcs
                .show_file(&analysis.compare_head, &path)?
                .and_then(|text| Changelog::parse(&text))
        } else {
            None
        };
        if dry_run {
            if self.changelog {
                outcome.changelog = Some(self.planned_changelog(analysis, &steps, previous));
            }
            for step in &steps {
                (self.progress)(&format!("[dry run] {}", step.describe()));
            }
            outcome.steps = steps
                .into_iter()
                .map(|s| (s, StepOutcome::Planned))
                .collect();
            (self.progress)(&format!("[dry run] would push {branch}"));
            return Ok(outcome);
        }
        if self.vcs.is_dirty()? {
            return Err(ReleaseError::Refused(
                "the work tree has uncommitted changes; commit or stash them first".into(),
            ));
        }
        (self.progress)(&format!(
            "checking out {branch} at {}",
            analysis.request.compare
        ));
        self.vcs.checkout_reset(&branch, &analysis.compare_head)?;
        if self.changelog {
            self.take_out_changelog(analysis)?;
        }
        let runner = StepRunner {
            vcs: self.vcs,
            confirm: self.confirm,
            progress: self.progress,
            interactive,
            conflicts: self.conflicts,
            analysis,
            selection: include,
        };
        outcome.steps = runner.run(steps)?;
        if self.changelog {
            let log = changelog::for_patch(
                previous,
                analysis,
                &branch,
                &outcome.steps,
                &releaser(self.vcs),
                &changelog::today(),
            );
            let file = commit_changelog(self.vcs, &log, None)?;
            (self.progress)(&format!("changelog rewritten: {}", file.path));
            outcome.changelog = Some(file);
        }
        (self.progress)(&format!("pushing {branch}"));
        // Leased on the analysed head: a push to the branch since then is not overwritten.
        self.vcs.push(
            &branch,
            PushMode::Expect(Some(analysis.compare_head.clone())),
        )?;
        outcome.pushed = true;
        Ok(outcome)
    }
}
