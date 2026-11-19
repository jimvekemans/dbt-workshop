//! Release, patch, and promotion execution flows on [`Hub`].

use std::sync::Arc;

use vdp_git::Vcs;
use vdp_release::{
    AnalysisKind, ConflictReport, Conflicts, PatchFlow, PatchOptions, PromoteOptions, Promotion,
    ReleaseFlow, ReleaseOptions, ScopeAction, ScopeCheck, apply_options, check_scope, local_name,
    remote_ref,
};

use super::{Actor, Hub, Stored};
use crate::api::{PatchCreate, PromotionCreate, ReleaseCreate};
use crate::error::HubError;
use crate::jobs::{Job, JobKind, JobResult};

impl Hub {
    fn executable(&self, id: &str, kind: AnalysisKind, actor: &Actor) -> Result<Stored, HubError> {
        self.require_clone()?;
        Self::require_identity(actor)?;
        let stored = self.stored(id)?;
        if stored.analysis.request.kind != kind {
            return Err(HubError::BadRequest(format!(
                "analysis {id} is a {:?} analysis",
                stored.analysis.request.kind
            )));
        }
        if self.is_stale(&stored.analysis) {
            return Err(HubError::Conflict(format!(
                "stale analysis: {} or {} moved since analysis {id}; analyse again",
                stored.analysis.request.source, stored.analysis.request.compare
            )));
        }
        Ok(stored)
    }

    /// Starts a job that cuts the release branch for the executable release analysis `create.analysis_id`.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet or `actor` has no git identity, or the analysis is stale; [`HubError::NotFound`] when there is no such analysis;
    /// [`HubError::BadRequest`] when it is not a release analysis or the branch is not a release branch; [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started.
    /// Failures of the flow itself end the job as failed.
    pub fn create_release(
        self: &Arc<Self>,
        create: &ReleaseCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        let stored = self.executable(&create.analysis_id, AnalysisKind::Release, &actor)?;
        let branch = self.rules.release_target(
            &create
                .branch
                .clone()
                .filter(|b| !b.trim().is_empty())
                .unwrap_or_else(|| stored.branch.clone()),
        )?;
        // Only the newest two release branches, never recreating one main already has, never an older new one.
        check_scope(
            &self.git(),
            &ScopeCheck {
                rules: &self.rules,
                branch: &branch,
                action: ScopeAction::Release { recreate: true },
            },
        )?;
        let options = ReleaseOptions {
            branch,
            force: create.force.unwrap_or(false),
            dry_run: create.dry_run.unwrap_or(false),
            interactive: false,
            resume: false,
            expect: create.expect.clone(),
            accept_losses: create.accept_losses.unwrap_or(false),
            changelog: true,
        };
        let title = format!(
            "{}release {}",
            if options.dry_run { "dry run: " } else { "" },
            options.branch
        );
        let apply = apply_options(
            create.on_conflict.unwrap_or_default(),
            create.strategy.unwrap_or_default(),
        );
        let hub = Arc::clone(self);
        self.jobs.start(JobKind::Release, title, move |log| {
            let git = hub.git_as(&actor);
            let progress = |m: &str| log.info(m);
            let timeout = hub.config.conflict_timeout;
            let resolve = |report: &ConflictReport| log.wait_for(report.clone(), timeout);
            let dir = hub.config.repo_dir.clone();
            let validator = hub.validator.clone();
            let check = move || validator.as_ref().map_or(Ok(()), |v| v(&dir));
            let validate: Option<&dyn Fn() -> Result<(), String>> = hub
                .validator
                .as_ref()
                .map(|_| &check as &dyn Fn() -> Result<(), String>);
            let outcome = ReleaseFlow {
                vcs: &git,
                rules: &hub.rules,
                validate,
                confirm: &|_, _, _| true,
                progress: &progress,
                conflicts: Conflicts {
                    options: apply,
                    resolve: Some(&resolve),
                },
            }
            .run(&stored.analysis, &stored.selection, &options);
            hub.tidy(&git, log);
            let outcome = outcome.map_err(|e| vdp_core::display_chain(&e))?;
            Ok(JobResult::Release { outcome })
        })
    }

    /// Starts a job that patches the release branch with the executable patch analysis `create.analysis_id`.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet or `actor` has no git identity, or the analysis is stale; [`HubError::NotFound`] when there is no such analysis;
    /// [`HubError::BadRequest`] when it is not a patch analysis or the branch is not a release branch; [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started.
    /// Failures of the flow itself end the job as failed.
    pub fn create_patch(
        self: &Arc<Self>,
        create: &PatchCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        let stored = self.executable(&create.analysis_id, AnalysisKind::Patch, &actor)?;
        check_scope(
            &self.git(),
            &ScopeCheck {
                rules: &self.rules,
                branch: &stored.branch,
                action: ScopeAction::Patch,
            },
        )?;
        let dry_run = create.dry_run.unwrap_or(false);
        let title = format!(
            "{}patch {}",
            if dry_run { "dry run: " } else { "" },
            stored.branch
        );
        let apply = apply_options(
            create.on_conflict.unwrap_or_default(),
            create.strategy.unwrap_or_default(),
        );
        let hub = Arc::clone(self);
        self.jobs.start(JobKind::Patch, title, move |log| {
            let git = hub.git_as(&actor);
            let progress = |m: &str| log.info(m);
            let timeout = hub.config.conflict_timeout;
            let resolve = |report: &ConflictReport| log.wait_for(report.clone(), timeout);
            let outcome = PatchFlow {
                vcs: &git,
                rules: &hub.rules,
                confirm: &|_, _, _| true,
                progress: &progress,
                conflicts: Conflicts {
                    options: apply,
                    resolve: Some(&resolve),
                },
                changelog: true,
            }
            .run(
                &stored.analysis,
                &stored.selection,
                &PatchOptions {
                    dry_run,
                    interactive: false,
                },
            );
            hub.tidy(&git, log);
            let outcome = outcome.map_err(|e| vdp_core::display_chain(&e))?;
            Ok(JobResult::Patch { outcome })
        })
    }

    /// Starts a job that promotes the release branch `create.branch` to main.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet or `actor` has no git identity; [`HubError::BadRequest`] when the branch is not a release branch;
    /// [`HubError::NotFound`] when origin does not have it; [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started.
    /// Failures of the promotion itself end the job as failed.
    pub fn create_promotion(
        self: &Arc<Self>,
        create: &PromotionCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        self.require_clone()?;
        Self::require_identity(&actor)?;
        let branch = self.rules.release_target(&create.branch)?;
        self.git()
            .rev_parse(&remote_ref(&branch))
            .map_err(|_| HubError::NotFound(format!("branch {branch} on origin")))?;
        check_scope(
            &self.git(),
            &ScopeCheck {
                rules: &self.rules,
                branch: &branch,
                action: ScopeAction::Promote,
            },
        )?;
        let options = PromoteOptions {
            kind: create.kind.unwrap_or_default().kind(),
            dry_run: create.dry_run.unwrap_or(false),
            expect_main: create.expect_main.clone(),
            accept_rollback: create.accept_rollback.unwrap_or(false),
            accept_losses: create.accept_losses.unwrap_or(false),
        };
        let dry_run = options.dry_run;
        let title = format!(
            "{}promote {branch} to {}",
            if dry_run { "dry run: " } else { "" },
            local_name(&self.config.release.to)
        );
        let hub = Arc::clone(self);
        self.jobs.start(JobKind::Promotion, title, move |log| {
            let git = hub.git_as(&actor);
            let progress = |m: &str| log.info(m);
            let timeout = hub.config.conflict_timeout;
            let resolve = |report: &ConflictReport| log.wait_for(report.clone(), timeout);
            let promoted = Promotion {
                vcs: &git,
                rules: &hub.rules,
                progress: &progress,
                conflicts: Conflicts {
                    options: vdp_git::ApplyOptions::default(),
                    resolve: Some(&resolve),
                },
            }
            .run(&branch, &options);
            hub.tidy(&git, log);
            let promoted = promoted.map_err(|e| vdp_core::display_chain(&e))?;
            Ok(JobResult::Promotion { promoted })
        })
    }
}
