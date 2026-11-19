//! Job polling, conflict resolution, and write flow orchestration on [`VsHub`].

use std::sync::Arc;

use vdp_git::{GitCli, Vcs};
use vdp_release::{
    ConflictReport, Conflicts, OnConflictChoice, Resolution, StrategyChoice, apply_options,
};
use vdp_vaultspeed_domain::Environment;
use vdp_vaultspeed_release::VsError;
use vdp_vaultspeed_release::flows::{self, FlowContext, RunControl};
use vdp_vaultspeed_release::preview::plan_preview;
use vdp_vaultspeed_release::status::{self, StatusInputs};

use vdp_release_hub::api::ConflictVersionsView;
use vdp_release_hub::jobs::{Job, JobKind, JobResult};
use vdp_release_hub::{Actor, HubError};

use super::{VsHub, hub_error};
use crate::api::{
    VsCommitCreate, VsJobResult, VsMergeCreate, VsReleaseCreate, VsResetCreate, VsRun,
};

impl VsHub {
    /// The jobs of the VaultSpeed side.
    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.list()
    }

    /// The job `id` with the steps after `after`.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such job.
    pub fn job(&self, id: &str, after: u32) -> Result<Job, HubError> {
        self.jobs.get(id, after)
    }

    /// Hands the release manager's decision to a job that waits at a conflict.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such job; [`HubError::Conflict`] when it is not waiting for a decision;
    /// [`HubError::BadRequest`] when `resolution` names a file that is not in the conflict.
    pub fn resolve(&self, id: &str, resolution: Resolution) -> Result<(), HubError> {
        self.jobs.resolve(id, resolution)
    }

    /// The three versions of a file in the conflict a job waits on.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when the job is not waiting for a decision; [`HubError::BadRequest`] when `path` is not a file of
    /// the conflict; [`HubError::Internal`] when git fails.
    pub fn conflict_versions(
        &self,
        id: &str,
        path: &str,
    ) -> Result<ConflictVersionsView, HubError> {
        let pending = self.jobs.pending(id)?;
        if !pending.files.iter().any(|f| f.path == path) {
            return Err(HubError::BadRequest(format!(
                "{path} is not a conflicting file"
            )));
        }
        let versions = self.git().conflict_versions(path, 256 * 1024)?;
        Ok(ConflictVersionsView {
            path: path.to_owned(),
            base: versions.base,
            branch: versions.branch,
            change: versions.change,
            binary: versions.binary,
            truncated: versions.truncated,
        })
    }

    // --- writing (as jobs) ------------------------------------------------------------------------------

    /// Refused before any job: zip branches (VaultSpeed's) and anything that is not main, an environment or a
    /// release branch.
    fn writable(&self, branch: &str) -> Result<(), HubError> {
        self.rules
            .guard()
            .check(branch)
            .map_err(|e| HubError::BadRequest(vdp_vaultspeed_domain::display_chain(&e)))
    }

    /// Runs `work` as a job with this side's conflict hand-off and progress, then tidies the clone.
    fn run<F>(
        self: &Arc<Self>,
        kind: JobKind,
        title: String,
        actor: Actor,
        run: VsRun,
        work: F,
    ) -> Result<Job, HubError>
    where
        F: FnOnce(&Self, &GitCli, &RunControl<'_>) -> Result<JobResult, VsError> + Send + 'static,
    {
        self.require_clone()?;
        let hub = Arc::clone(self);
        self.jobs.start(kind, title, move |log| {
            let git = hub.git_as(&actor);
            let timeout = hub.config.conflict_timeout;
            let resolve = |report: &ConflictReport| log.wait_for(report.clone(), timeout);
            let progress = |message: &str| log.info(message);
            let control = RunControl {
                dry_run: run.dry_run.unwrap_or(false),
                conflicts: Conflicts {
                    options: apply_options(
                        run.on_conflict.unwrap_or(OnConflictChoice::Pause),
                        run.strategy.unwrap_or(StrategyChoice::TheirsHunks),
                    ),
                    resolve: Some(&resolve),
                },
                progress: &progress,
            };
            let result = work(&hub, &git, &control);
            hub.tidy(&git, log);
            result.map_err(|e| vdp_vaultspeed_domain::display_chain(&e))
        })
    }

    /// Starts a job that builds the release branch of `create.request.drop` (or only previews it).
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when the DROP is not a release name; [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running. Failures of the flow itself end the job as failed (a
    /// conflict the release manager must decide waits as a pending job).
    pub fn create_release(
        self: &Arc<Self>,
        create: &VsReleaseCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        let branch = self
            .rules
            .release_branch(&create.request.drop)
            .map_err(hub_error)?;
        let run = create.run.unwrap_or_default();
        let title = format!(
            "{} {branch}",
            if run.dry_run == Some(true) {
                "preview"
            } else {
                "build"
            }
        );
        let request = create.request.clone();
        let expect = create.expect.clone();
        self.run(
            JobKind::Other("vs_release".into()),
            title,
            actor,
            run,
            move |hub, git, control| {
                let outcome = flows::build_release(
                    &FlowContext {
                        vcs: git,
                        rules: &hub.rules,
                        expect: expect.as_deref(),
                        control,
                    },
                    &request,
                )?;
                Ok(VsJobResult::Release { outcome }.into_result())
            },
        )
    }

    /// Starts a job that merges `create.request.sources` into an environment (or only previews it).
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running. Failures of the flow itself end the job as failed (a
    /// conflict the release manager must decide waits as a pending job).
    pub fn create_merge(
        self: &Arc<Self>,
        create: &VsMergeCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        let run = create.run.unwrap_or_default();
        let env = create.request.environment.clone();
        let title = format!(
            "{} {} into {env}",
            if run.dry_run == Some(true) {
                "preview"
            } else {
                "merge"
            },
            create.request.sources.join(", ")
        );
        let request = create.request.clone();
        let expect = create.expect.clone();
        let plan = create.plan.unwrap_or(false);
        self.run(
            JobKind::Other("vs_merge".into()),
            title,
            actor,
            run,
            move |hub, git, control| {
                git.fetch_all()?;
                // Which zips the environment below deployed (for the warnings), as origin has it now.
                let known = status::status(
                    git,
                    StatusInputs {
                        rules: &hub.rules,
                        zips: &hub.zips,
                        tickets: &hub.tickets,
                        tracker: None,
                    },
                )?;
                let deployed_below = |zip: &str, lower: &str| {
                    known
                        .zips
                        .iter()
                        .find(|z| z.name == zip)
                        .and_then(|z| z.environments.get(lower))
                        .is_some_and(|e| e.deployed.is_some())
                };
                let outcome = flows::merge_into_environment(
                    &FlowContext {
                        vcs: git,
                        rules: &hub.rules,
                        expect: expect.as_deref(),
                        control,
                    },
                    &request,
                    &deployed_below,
                )?;
                let plan = if plan {
                    (control.progress)("planning what CI would deploy (offline)");
                    let environment: Environment =
                        env.parse().map_err(|e| VsError::Plan(format!("{e}")))?;
                    Some(plan_preview(
                        &hub.config.repo_dir,
                        &hub.config.vaultspeed,
                        environment,
                    )?)
                } else {
                    None
                };
                Ok(VsJobResult::Merge { outcome, plan }.into_result())
            },
        )
    }

    /// Starts a job that commits direct changes to a branch (or only previews it).
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when the branch is not `main`, an environment or a release branch; [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running. Failures of the flow itself end the job as failed (a
    /// conflict the release manager must decide waits as a pending job).
    pub fn create_commit(
        self: &Arc<Self>,
        create: &VsCommitCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        self.writable(&create.request.branch)?;
        let dry_run = create.dry_run.unwrap_or(false);
        let title = format!(
            "{} on {}",
            if dry_run { "preview a fix" } else { "fix" },
            create.request.branch
        );
        let request = create.request.clone();
        let expect = create.expect.clone();
        let run = VsRun {
            dry_run: Some(dry_run),
            ..VsRun::default()
        };
        self.run(
            JobKind::Other("vs_commit".into()),
            title,
            actor,
            run,
            move |hub, git, control| {
                let outcome = flows::commit_files(
                    &FlowContext {
                        vcs: git,
                        rules: &hub.rules,
                        expect: expect.as_deref(),
                        control,
                    },
                    &request,
                )?;
                Ok(VsJobResult::Commit { outcome }.into_result())
            },
        )
    }

    /// Starts a job that resets a branch to an earlier commit of its own (or only previews it).
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when the branch is not `main`, an environment or a release branch; [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running. Failures of the flow itself end the job as failed (a
    /// conflict the release manager must decide waits as a pending job).
    pub fn create_reset(
        self: &Arc<Self>,
        create: &VsResetCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        self.writable(&create.request.branch)?;
        let dry_run = create.dry_run.unwrap_or(false);
        let title = format!(
            "{} {} to {}",
            if dry_run {
                "preview a reset of"
            } else {
                "reset"
            },
            create.request.branch,
            create.request.target.chars().take(8).collect::<String>()
        );
        let request = create.request.clone();
        let expect = create.expect.clone();
        let run = VsRun {
            dry_run: Some(dry_run),
            ..VsRun::default()
        };
        self.run(
            JobKind::Other("vs_reset".into()),
            title,
            actor,
            run,
            move |hub, git, control| {
                let outcome = flows::reset_branch(
                    &FlowContext {
                        vcs: git,
                        rules: &hub.rules,
                        expect: expect.as_deref(),
                        control,
                    },
                    &request,
                )?;
                Ok(VsJobResult::Reset { outcome }.into_result())
            },
        )
    }
}
