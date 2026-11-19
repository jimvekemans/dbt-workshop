//! Executing a release: a new branch from the source, with the selected tickets reverted.

use std::collections::BTreeSet;
use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use vdp_git::{Applied, PushMode, Vcs};

use crate::changelog::{self, Changelog};
use crate::conflicts::{self, Conflicts, ResolutionAction};

use crate::analysis::Analysis;
use crate::{BranchRules, OriginState, ReleaseError, ReleaseOnly, origin_state, release_only};

/// How to execute.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent switches, as the CLI flags"
)]
#[derive(Debug, Clone)]
pub struct ReleaseOptions {
    /// The release branch, e.g. `release/2026_DROP05`.
    pub branch: String,
    /// Replace an existing local branch and force-push (with lease).
    pub force: bool,
    pub dry_run: bool,
    pub interactive: bool,
    /// Continue on an existing local branch (the CLI: re-run after resolving a conflict by hand). Otherwise the
    /// local branch is always recreated from the analysed source head.
    pub resume: bool,
    /// What the preview saw on origin for this branch: refused if origin moved since, and the push leases on it.
    pub expect: Option<OriginState>,
    /// Recreating drops commits only the old branch has: they were listed and accepted.
    pub accept_losses: bool,
    /// Commit `changelog/<fix version>.md` as the branch's last commit.
    pub changelog: bool,
}

/// A changelog file as committed (or, in a dry run, as it would be).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangelogFile {
    pub path: String,
    pub content: String,
}

/// Who is releasing, for the changelog.
pub(crate) fn releaser(vcs: &dyn Vcs) -> String {
    vcs.identity()
        .ok()
        .flatten()
        .map_or_else(|| "unknown".to_owned(), |i| i.name)
}

/// Commits the changelog (with the other releases' changelogs from `carry_from`, so main keeps them across
/// the reset of the next new promotion).
pub(crate) fn commit_changelog(
    vcs: &dyn Vcs,
    log: &Changelog,
    carry_from: Option<&str>,
) -> Result<ChangelogFile, ReleaseError> {
    let path = changelog::path(&log.release);
    let content = log.render();
    let mut files: Vec<(String, String)> = Vec::new();
    if let Some(rev) = carry_from {
        for other in vcs.files_at(rev, "changelog/")? {
            if other != path
                && let Some(text) = vcs.show_file(rev, &other)?
            {
                files.push((other, text));
            }
        }
    }
    files.push((path.clone(), content.clone()));
    vcs.commit_files(&files, &changelog::commit_message(&log.release))?;
    Ok(ChangelogFile { path, content })
}

/// One planned git action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    /// Revert a merge of a ticket on the release branch.
    Revert {
        ticket: String,
        commit: String,
        subject: String,
    },
    /// Cherry-pick a revert that is already on the compare branch, for out-of-scope tickets.
    CherryPick {
        tickets: Vec<String>,
        commit: String,
        subject: String,
    },
    /// Revert a revert on a release branch: the ticket is back in (patch).
    ReEnable {
        ticket: String,
        commit: String,
        subject: String,
    },
    /// Cherry-pick a ticket's merge onto a release branch (patch).
    Pick {
        ticket: String,
        commit: String,
        subject: String,
    },
}

impl Step {
    /// The hash of the commit the step applies.
    pub fn commit(&self) -> &str {
        match self {
            Self::Revert { commit, .. }
            | Self::CherryPick { commit, .. }
            | Self::ReEnable { commit, .. }
            | Self::Pick { commit, .. } => commit,
        }
    }

    /// A one-line description for logs and prompts, for example `revert INTEL-1 (abc1234)`.
    pub fn describe(&self) -> String {
        let short = &self.commit()[..self.commit().len().min(7)];
        match self {
            Self::Revert { ticket, .. } => format!("revert {ticket} ({short})"),
            Self::CherryPick { tickets, .. } => {
                format!("cherry-pick the revert of {} ({short})", tickets.join(", "))
            }
            Self::ReEnable { ticket, .. } => {
                format!("re-enable {ticket}: revert its revert ({short})")
            }
            Self::Pick { ticket, .. } => format!("cherry-pick {ticket} ({short})"),
        }
    }
}

/// What happened to a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Applied,
    /// Applied after resolving conflicts for the change (its version of a file, or its deletion); the paths are
    /// in the progress log.
    Resolved,
    /// Already in effect; nothing committed.
    Empty,
    /// Declined at the interactive prompt.
    Skipped,
    /// Dry run.
    Planned,
}

/// What a step is about.
impl Step {
    /// The tickets a step is about.
    pub fn tickets(&self) -> Vec<&str> {
        match self {
            Self::Revert { ticket, .. }
            | Self::ReEnable { ticket, .. }
            | Self::Pick { ticket, .. } => {
                vec![ticket.as_str()]
            }
            Self::CherryPick { tickets, .. } => tickets.iter().map(String::as_str).collect(),
        }
    }
}

/// The executed (or, in a dry run, planned) steps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Outcome {
    pub branch: String,
    pub dry_run: bool,
    /// The branch on origin before this run (pass it back to execute exactly what was previewed).
    pub origin: Option<OriginState>,
    /// Commits only the old branch has, which recreating it drops (fixes made there directly).
    pub losses: Vec<ReleaseOnly>,
    /// The changelog committed last (in a dry run: as it would be).
    pub changelog: Option<ChangelogFile>,
    pub steps: Vec<(Step, StepOutcome)>,
    pub validated: bool,
    pub pushed: bool,
}

/// The git actions of a release with `revert` as the selection: reverts newest first, then the compare
/// branch's reverts of out-of-scope tickets, oldest first and once per commit.
pub fn release_steps(analysis: &Analysis, revert: &BTreeSet<String>) -> Vec<Step> {
    let mut merges: Vec<(&str, &crate::CommitInfo)> = analysis
        .tickets
        .iter()
        .filter(|t| revert.contains(&t.id))
        .flat_map(|t| t.merges.iter().map(move |m| (t.id.as_str(), m)))
        .collect();
    merges.sort_by_key(|(_, m)| std::cmp::Reverse(m.position));
    let mut steps: Vec<Step> = merges
        .into_iter()
        .map(|(ticket, merge)| Step::Revert {
            ticket: ticket.to_owned(),
            commit: merge.hash.clone(),
            subject: merge.subject.clone(),
        })
        .collect();
    for revert_commit in &analysis.compare_reverts {
        // Out of scope by policy, and not already reverted from the source by this selection.
        let wanted = revert_commit.tickets.iter().any(|id| {
            !revert.contains(id)
                && analysis
                    .ticket(id)
                    .is_some_and(|t| t.reverted_by_policy(analysis.keep_empty))
        });
        if wanted {
            steps.push(Step::CherryPick {
                tickets: revert_commit.tickets.clone(),
                commit: revert_commit.commit.hash.clone(),
                subject: revert_commit.commit.subject.clone(),
            });
        }
    }
    steps
}

/// Runs steps that change the work tree.
pub(crate) struct StepRunner<'a> {
    pub vcs: &'a dyn Vcs,
    pub confirm: &'a dyn Fn(&Step, usize, usize) -> bool,
    pub progress: &'a dyn Fn(&str),
    pub interactive: bool,
    pub conflicts: Conflicts<'a>,
    /// For conflict reports: the analysis and the selection being executed.
    pub analysis: &'a Analysis,
    pub selection: &'a BTreeSet<String>,
}

impl StepRunner<'_> {
    pub(crate) fn run(&self, steps: Vec<Step>) -> Result<Vec<(Step, StepOutcome)>, ReleaseError> {
        let total = steps.len();
        let mut done = Vec::with_capacity(total);
        for (index, step) in steps.into_iter().enumerate() {
            if self.interactive && !(self.confirm)(&step, index + 1, total) {
                (self.progress)(&format!("skipped: {}", step.describe()));
                done.push((step, StepOutcome::Skipped));
                continue;
            }
            let outcome = self.apply(&step)?;
            done.push((step, outcome));
        }
        Ok(done)
    }

    fn apply(&self, step: &Step) -> Result<StepOutcome, ReleaseError> {
        let failed = |e: vdp_git::GitError| ReleaseError::Apply {
            action: step.describe(),
            detail: vdp_core::display_chain(&e),
        };
        let operation = conflicts::operation(step);
        let options = self.conflicts.options;
        let mut applied = match operation {
            vdp_git::Operation::Revert => self.vcs.revert_with(step.commit(), options),
            _ => self.vcs.cherry_pick_with(step.commit(), options),
        }
        .map_err(failed)?;
        let mut decided: Vec<String> = Vec::new();
        loop {
            let (outcome, note) = match applied {
                Applied::Committed if decided.is_empty() => {
                    (StepOutcome::Applied, "done".to_owned())
                }
                Applied::Committed => (
                    StepOutcome::Resolved,
                    format!("done; conflicts decided: {}", decided.join(", ")),
                ),
                Applied::Empty if !decided.is_empty() => (
                    StepOutcome::Resolved,
                    format!(
                        "conflicts decided ({}); nothing else to change",
                        decided.join(", ")
                    ),
                ),
                Applied::Empty => (StepOutcome::Empty, "already in effect".to_owned()),
                Applied::Resolved(paths) => (
                    StepOutcome::Resolved,
                    format!(
                        "done; conflicts resolved for the {} side in {}",
                        if operation == vdp_git::Operation::Revert {
                            "revert"
                        } else {
                            "picked"
                        },
                        paths.join(", ")
                    ),
                ),
                Applied::Conflicted(conflict) => {
                    let Some(resolve) = self.conflicts.resolve else {
                        let _ = self.vcs.abort_op(operation);
                        return Err(ReleaseError::Apply {
                            action: step.describe(),
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
                    let report =
                        conflicts::report(self.vcs, self.analysis, self.selection, step, &conflict);
                    (self.progress)(&format!(
                        "{}: conflict in {} file(s); waiting for a decision",
                        step.describe(),
                        report.files.len()
                    ));
                    let resolution = resolve(&report);
                    match resolution.action {
                        ResolutionAction::Abort => {
                            self.vcs.abort_op(operation).map_err(failed)?;
                            return Err(ReleaseError::Aborted(format!(
                                "stopped at {}; nothing was pushed",
                                step.describe()
                            )));
                        }
                        ResolutionAction::SkipStep => {
                            self.vcs.skip_op(operation).map_err(failed)?;
                            (self.progress)(&format!(
                                "{}: left out (the branch keeps this ticket as it is)",
                                step.describe()
                            ));
                            return Ok(StepOutcome::Skipped);
                        }
                        ResolutionAction::Continue => {
                            for (path, choice) in &resolution.choices {
                                self.vcs
                                    .resolve_path(path, (*choice).into())
                                    .map_err(failed)?;
                                decided.push(format!("{path}: {choice:?}").to_lowercase());
                            }
                            applied = self
                                .vcs
                                .continue_op(operation, step.commit())
                                .map_err(failed)?;
                            continue;
                        }
                    }
                }
            };
            (self.progress)(&format!("{}: {note}", step.describe()));
            return Ok(outcome);
        }
    }
}

/// The release flow over its ports.
pub struct ReleaseFlow<'a> {
    pub vcs: &'a dyn Vcs,
    /// The branch must be a release branch (never the source).
    pub rules: &'a BranchRules,
    /// Runs before the push (dbt validation in the CLI); `Err` with the reason stops the push.
    pub validate: Option<&'a dyn Fn() -> Result<(), String>>,
    /// Asks whether to apply a step (interactive mode).
    pub confirm: &'a dyn Fn(&Step, usize, usize) -> bool,
    pub progress: &'a dyn Fn(&str),
    /// Conflict handling (default: resolve for the change being applied, as before).
    pub conflicts: Conflicts<'a>,
}

impl Debug for ReleaseFlow<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReleaseFlow")
            .field("vcs", &self.vcs)
            .finish_non_exhaustive()
    }
}

/// Refuses to act on an analysis whose refs have moved since.
pub(crate) fn check_fresh(
    vcs: &dyn Vcs,
    analysis: &Analysis,
    progress: &dyn Fn(&str),
) -> Result<(), ReleaseError> {
    if let Err(error) = vcs.fetch_all() {
        progress(&format!(
            "fetch failed (continuing with local refs): {}",
            vdp_core::display_chain(&error)
        ));
    }
    let gone = |name: &str| {
        ReleaseError::Stale(format!(
            "{name} no longer exists (deleted or renamed since the analysis); analyse again"
        ))
    };
    let source = vcs
        .rev_parse(&analysis.request.source)
        .map_err(|_| gone(&analysis.request.source))?;
    let compare = vcs
        .rev_parse(&analysis.request.compare)
        .map_err(|_| gone(&analysis.request.compare))?;
    if source != analysis.source_head || compare != analysis.compare_head {
        return Err(ReleaseError::Stale(format!(
            "{} or {} moved since the analysis; analyse again",
            analysis.request.source, analysis.request.compare
        )));
    }
    Ok(())
}

impl ReleaseFlow<'_> {
    /// Origin's branch now: refused when it moved since the preview; what recreating it would lose.
    fn origin(
        &self,
        analysis: &Analysis,
        branch: &str,
        options: &ReleaseOptions,
        outcome: &mut Outcome,
    ) -> Result<OriginState, ReleaseError> {
        let origin = origin_state(self.vcs, branch)?;
        if let Some(expected) = &options.expect
            && expected.head != origin.head
        {
            return Err(ReleaseError::Stale(format!(
                "{branch} on origin moved since the preview; preview again"
            )));
        }
        if origin.head.is_some() {
            outcome.losses = release_only(
                self.vcs,
                &crate::ReleaseOnlyRequest {
                    source: &analysis.source_head,
                    head: branch,
                    ticket_prefix: &self.rules.ticket_prefix,
                },
            )?;
        }
        outcome.origin = Some(origin.clone());
        Ok(origin)
    }

    /// The dry run: the steps, what would be lost, the changelog.
    fn plan(
        &self,
        analysis: &Analysis,
        steps: Vec<Step>,
        options: &ReleaseOptions,
        outcome: &mut Outcome,
    ) {
        let branch = outcome.branch.clone();
        if outcome.origin.as_ref().is_some_and(|o| o.head.is_some()) {
            (self.progress)(&format!(
                "[dry run] {branch} exists on origin: executing needs \"recreate\" (push with lease)"
            ));
        }
        for lost in &outcome.losses {
            (self.progress)(&format!(
                "[dry run] recreating drops {} {} (only {branch} has it)",
                lost.commit.short, lost.commit.subject
            ));
        }
        for step in &steps {
            (self.progress)(&format!("[dry run] {}", step.describe()));
        }
        outcome.steps = steps
            .into_iter()
            .map(|s| (s, StepOutcome::Planned))
            .collect();
        if options.changelog {
            let log = changelog::for_release(
                analysis,
                &branch,
                &outcome.steps,
                &releaser(self.vcs),
                &changelog::today(),
            );
            outcome.changelog = Some(ChangelogFile {
                path: changelog::path(&log.release),
                content: log.render(),
            });
        }
        (self.progress)(&format!("[dry run] would push {branch}"));
    }

    /// Cuts the release branch for `analysis`: reverts the tickets in `revert` (newest first), carries over the compare
    /// branch's reverts, optionally commits the changelog, validates and pushes. With `options.dry_run` it only plans.
    ///
    /// # Errors
    ///
    /// [`ReleaseError::Invalid`] when the branch is not a release branch or the selection is invalid;
    /// [`ReleaseError::Stale`] when a ref moved or disappeared since the analysis or origin's branch moved since the preview;
    /// [`ReleaseError::Refused`] when the branch exists on origin and `force` is off, dropped commits were not accepted, or
    /// the work tree is dirty; [`ReleaseError::Apply`] when a step fails; [`ReleaseError::Aborted`] when the release manager
    /// stops at a conflict; [`ReleaseError::Validation`] when the validation hook fails (nothing is pushed);
    /// [`ReleaseError::Git`] when another git operation fails (including a rejected push).
    pub fn run(
        &self,
        analysis: &Analysis,
        revert: &BTreeSet<String>,
        options: &ReleaseOptions,
    ) -> Result<Outcome, ReleaseError> {
        let branch = self.rules.release_target(&options.branch)?;
        analysis.validate_selection(revert)?;
        let steps = release_steps(analysis, revert);
        let mut outcome = Outcome {
            branch: branch.clone(),
            dry_run: options.dry_run,
            ..Outcome::default()
        };
        check_fresh(self.vcs, analysis, self.progress)?;
        let origin = self.origin(analysis, &branch, options, &mut outcome)?;
        if options.dry_run {
            self.plan(analysis, steps, options, &mut outcome);
            return Ok(outcome);
        }
        if origin.head.is_some() && !options.force {
            return Err(ReleaseError::Refused(format!(
                "{branch} exists on origin: recreate it (replaces it, with a lease) or patch it"
            )));
        }
        if !outcome.losses.is_empty() && !options.accept_losses {
            return Err(ReleaseError::Refused(format!(
                "recreating {branch} drops {} commit(s) only it has; accept that explicitly",
                outcome.losses.len()
            )));
        }
        if self.vcs.is_dirty()? {
            return Err(ReleaseError::Refused(
                "the work tree has uncommitted changes; commit or stash them first".into(),
            ));
        }
        let exists = self.vcs.branch_exists(&branch)?;
        if exists && options.resume && !options.force {
            (self.progress)(&format!("branch {branch} exists; continuing on it"));
            self.vcs.checkout(&branch)?;
        } else {
            (self.progress)(&format!(
                "creating {} from {}",
                branch, analysis.request.source
            ));
            self.vcs
                .create_branch(&branch, &analysis.source_head, exists)?;
        }
        let runner = StepRunner {
            vcs: self.vcs,
            confirm: self.confirm,
            progress: self.progress,
            interactive: options.interactive,
            conflicts: self.conflicts,
            analysis,
            selection: revert,
        };
        outcome.steps = runner.run(steps)?;
        if options.changelog {
            let log = changelog::for_release(
                analysis,
                &branch,
                &outcome.steps,
                &releaser(self.vcs),
                &changelog::today(),
            );
            let file = commit_changelog(self.vcs, &log, Some(&analysis.request.compare))?;
            (self.progress)(&format!("changelog committed: {}", file.path));
            outcome.changelog = Some(file);
        }
        if let Some(validate) = self.validate {
            (self.progress)("validating");
            validate().map_err(ReleaseError::Validation)?;
            outcome.validated = true;
        }
        // Exactly what origin had at the start (or "must not exist"): nobody's push in between is overwritten.
        let mode = PushMode::Expect(origin.head.clone());
        (self.progress)(&format!("pushing {branch}"));
        self.vcs.push(&branch, mode)?;
        outcome.pushed = true;
        Ok(outcome)
    }
}
