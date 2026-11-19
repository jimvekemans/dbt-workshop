//! The VaultSpeed side of the hub: a clone of the vaultspeed-automation-framework repository with its own jobs.
//!
//! Every flow is `vdp_vaultspeed_release`'s (the CLI runs the same code); this module adds the clone's lifecycle
//! (clone, sync, repair, re-clone), runs flows as jobs (with conflicts put to the release manager), caches the
//! status, and serves files and history for the editors. After every job the clone goes back to a detached HEAD
//! at `main` without local branches.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use vdp_git::{GitCli, Vcs};
use vdp_jira::IssueTracker;
use vdp_release::changes::{FileChanges, FileDiff, file_changes};
use vdp_release::{
    ConflictReport, Conflicts, OnConflictChoice, Resolution, StrategyChoice, apply_options,
};
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed_domain::Environment;
use vdp_vaultspeed_release::changes as vs_changes;
use vdp_vaultspeed_release::flows::{self, RunControl};
use vdp_vaultspeed_release::preview::plan_preview;
use vdp_vaultspeed_release::status::{self, CommitRef, Status, TicketCache};
use vdp_vaultspeed_release::zips::ZipCache;
use vdp_vaultspeed_release::{VsError, VsReleaseSettings, VsRules};

use vdp_release_hub::api::{CloneRequest, ConflictVersionsView, RepoStatus};
use vdp_release_hub::jobs::{Job, JobKind, JobLog, JobResult, Jobs};
use vdp_release_hub::{Actor, GitLab, HubError, repo_status_of};

use crate::api::{
    VsCommitCreate, VsFile, VsJobResult, VsMergeCreate, VsReleaseCreate, VsResetCreate, VsRun,
    VsSummary,
};

/// The VaultSpeed side's configuration.
#[derive(Debug, Clone)]
pub struct VsConfig {
    pub repo_dir: PathBuf,
    pub repo_url: String,
    pub gitlab: Option<GitLab>,
    pub release: VsReleaseSettings,
    pub vaultspeed: VaultspeedSettings,
    pub keep_jobs: usize,
    pub git_timeout: Option<Duration>,
    pub conflict_timeout: Duration,
    pub state_dir: Option<PathBuf>,
}

pub struct VsHub {
    config: VsConfig,
    rules: VsRules,
    jobs: Arc<Jobs>,
    tracker: Option<Arc<dyn IssueTracker>>,
    zips: ZipCache,
    tickets: TicketCache,
    /// The last status and the refs it was computed from.
    status: Mutex<Option<(String, Status)>>,
}

impl std::fmt::Debug for VsHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VsHub")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

fn hub_error(error: VsError) -> HubError {
    match error {
        VsError::Stale(m) => HubError::Conflict(m),
        VsError::Refused(m) | VsError::Invalid(m) => HubError::BadRequest(m),
        other => HubError::Internal(other.to_string()),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl VsHub {
    pub fn new(config: VsConfig, tracker: Option<Arc<dyn IssueTracker>>) -> Arc<Self> {
        Arc::new(Self {
            jobs: Arc::new(Jobs::new(config.keep_jobs, config.state_dir.clone())),
            rules: VsRules::new(config.release.clone()),
            config,
            tracker,
            zips: ZipCache::default(),
            tickets: TicketCache::default(),
            status: Mutex::new(None),
        })
    }

    pub fn config(&self) -> &VsConfig {
        &self.config
    }

    pub fn rules(&self) -> &VsRules {
        &self.rules
    }

    fn git(&self) -> GitCli {
        GitCli::new(&self.config.repo_dir)
            .with_guard(self.rules.guard())
            .with_timeout(self.config.git_timeout)
    }

    fn git_as(&self, actor: &Actor) -> GitCli {
        self.git()
            .with_identity(actor.identity.clone())
            .with_push_target(actor.push.clone())
    }

    fn require_clone(&self) -> Result<GitCli, HubError> {
        let git = self.git();
        if git.is_repository() {
            Ok(git)
        } else {
            Err(HubError::Conflict(
                "the VaultSpeed repository is not cloned yet".into(),
            ))
        }
    }

    pub fn repo_status(&self) -> RepoStatus {
        // No branch is special here (every flow starts from origin): any local branch is a leftover.
        repo_status_of(&self.config.repo_dir, self.jobs.active().is_some(), "")
    }

    pub fn summary(&self) -> VsSummary {
        let s = &self.config.release;
        VsSummary {
            repo: self.repo_status(),
            repo_url: self.config.repo_url.clone(),
            active_job: self.jobs.active(),
            environments: s.environments.clone(),
            direct_merge_environments: s.direct_merge_environments.clone(),
            main: s.main.clone(),
            zip_prefix: s.zip_prefix.clone(),
            release_prefix: s.release_prefix.clone(),
            pml_dir: s.pml_dir.clone(),
        }
    }

    /// The branch whose rights a write needs (a release branch: the `feature/*` rule).
    pub fn rights_branch(&self, branch: &str) -> String {
        if self.rules.is_release_branch(branch) {
            format!("{}*", self.config.release.release_prefix)
        } else {
            branch.to_owned()
        }
    }

    /// The branches whose rights the UI shows: main, each environment, release branches, zip branches.
    pub fn rights_samples(&self) -> Vec<(String, String)> {
        let s = &self.config.release;
        let mut samples = vec![(s.main.clone(), s.main.clone())];
        samples.extend(s.environments.iter().map(|e| (e.clone(), e.clone())));
        samples.push((
            format!("{}*", s.release_prefix),
            format!("{}2099_DROP99.1", s.release_prefix),
        ));
        samples.push((
            format!("{}*", s.zip_prefix),
            format!("{}99999_FMC", s.zip_prefix),
        ));
        samples
    }

    // --- the clone ---------------------------------------------------------------------------------------

    pub fn clone_repo(self: &Arc<Self>, request: &CloneRequest) -> Result<Job, HubError> {
        let url = request
            .url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| self.config.repo_url.clone());
        if url.trim().is_empty() {
            return Err(HubError::BadRequest("no repository URL".into()));
        }
        if self.git().is_repository() {
            return Err(HubError::Conflict(
                "the VaultSpeed repository is already cloned: sync it instead".into(),
            ));
        }
        let dir = &self.config.repo_dir;
        if std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some()) {
            return Err(HubError::Conflict(format!(
                "{} is not empty and not a git clone",
                dir.display()
            )));
        }
        let hub = Arc::clone(self);
        self.jobs
            .start(JobKind::Clone, format!("clone {url}"), move |log| {
                let git = hub.git();
                log.info(format!(
                    "cloning {url} into {}",
                    hub.config.repo_dir.display()
                ));
                git.clone_from(&url).map_err(|e| e.to_string())?;
                hub.detach(&git, log)?;
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

    pub fn reclone(self: &Arc<Self>, request: &CloneRequest) -> Result<Job, HubError> {
        let url = request
            .url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .or_else(|| self.git().origin_url().ok().flatten())
            .unwrap_or_else(|| self.config.repo_url.clone());
        let hub = Arc::clone(self);
        self.jobs
            .start(JobKind::Clone, format!("re-clone {url}"), move |log| {
                let dir = &hub.config.repo_dir;
                log.info(format!("removing the clone in {}", dir.display()));
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let removed = if path.is_dir() {
                            std::fs::remove_dir_all(&path)
                        } else {
                            std::fs::remove_file(&path)
                        };
                        removed.map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
                    }
                }
                let git = hub.git();
                git.clone_from(&url).map_err(|e| e.to_string())?;
                hub.detach(&git, log)?;
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

    pub fn sync(self: &Arc<Self>) -> Result<Job, HubError> {
        self.require_clone()?;
        let hub = Arc::clone(self);
        self.jobs
            .start(JobKind::Sync, "sync with origin", move |log| {
                log.info("fetching origin (all branches, pruned, tags)");
                hub.git().fetch_all().map_err(|e| e.to_string())?;
                hub.tickets.clear();
                log.info("up to date");
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

    pub fn repair(self: &Arc<Self>) -> Result<Job, HubError> {
        self.require_clone()?;
        let hub = Arc::clone(self);
        self.jobs
            .start(JobKind::Repair, "repair the clone", move |log| {
                let git = GitCli::new(&hub.config.repo_dir).with_timeout(hub.config.git_timeout);
                let main = format!("origin/{}", hub.config.release.main);
                for action in git.repair(&main).map_err(|e| e.to_string())? {
                    log.info(action);
                }
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

    /// HEAD detached at `main`, no local branches: every flow starts its branch from origin.
    fn detach(&self, git: &GitCli, log: &JobLog) -> Result<(), String> {
        let main = format!("origin/{}", self.config.release.main);
        git.detach_at(&main).map_err(|e| e.to_string())?;
        let deleted = git.delete_local_branches().map_err(|e| e.to_string())?;
        if !deleted.is_empty() {
            log.info(format!("local branches removed: {}", deleted.join(", ")));
        }
        Ok(())
    }

    /// After a job: back to a detached HEAD, unless something was left in progress (health shows it).
    fn tidy(&self, git: &GitCli, log: &JobLog) {
        let state = git.workspace_state();
        if state.operation.is_some() || state.index_lock || !state.dirty.is_empty() {
            log.warn("the clone is left as it is: check its health (repair resets it)");
            return;
        }
        if let Err(error) = self.detach(git, log) {
            log.warn(format!("could not tidy the clone: {error}"));
        }
        *lock(&self.status) = None;
    }

    // --- reading ----------------------------------------------------------------------------------------

    /// Zips per environment, release branches and tickets, from origin as last fetched (cached per refs).
    pub fn status(&self, refresh_jira: bool) -> Result<Status, HubError> {
        let git = self.require_clone()?;
        if refresh_jira {
            self.tickets.clear();
            *lock(&self.status) = None;
        }
        let refs = git
            .remote_branches("")
            .map_err(HubError::from)?
            .iter()
            .map(|b| format!("{}={}", b.name, b.head))
            .collect::<Vec<_>>()
            .join(",");
        if let Some((seen, status)) = lock(&self.status).as_ref()
            && *seen == refs
        {
            return Ok(status.clone());
        }
        let status = status::status(
            &git,
            &self.rules,
            &self.zips,
            &self.tickets,
            self.tracker.as_deref(),
        )
        .map_err(hub_error)?;
        *lock(&self.status) = Some((refs, status.clone()));
        Ok(status)
    }

    fn readable(&self, branch: &str) -> Result<(), HubError> {
        let ok = branch == self.config.release.main
            || self.rules.is_environment(branch)
            || self.rules.is_release_branch(branch)
            || self.rules.is_zip_branch(branch);
        if ok && !branch.contains("..") {
            Ok(())
        } else {
            Err(HubError::BadRequest(format!(
                "{branch} is not a branch shown here"
            )))
        }
    }

    /// A file as origin has it (for the editors).
    pub fn file(&self, branch: &str, path: &str) -> Result<VsFile, HubError> {
        self.readable(branch)?;
        let git = self.require_clone()?;
        let rev = format!("origin/{branch}");
        let head = git
            .rev_parse(&rev)
            .map_err(|_| HubError::NotFound(format!("{branch} on origin")))?;
        Ok(VsFile {
            branch: branch.to_owned(),
            path: path.to_owned(),
            content: git.show_file(&rev, path)?,
            head,
        })
    }

    /// The files under `dir` on a branch.
    pub fn tree(&self, branch: &str, dir: &str) -> Result<Vec<String>, HubError> {
        self.readable(branch)?;
        Ok(self
            .require_clone()?
            .files_at(&format!("origin/{branch}"), dir)?)
    }

    /// A branch's own recent history (for the reset picker).
    pub fn history(&self, branch: &str, limit: usize) -> Result<Vec<CommitRef>, HubError> {
        self.readable(branch)?;
        Ok(self
            .require_clone()?
            .recent(&format!("origin/{branch}"), limit.clamp(1, 200), true)?
            .iter()
            .map(CommitRef::from)
            .collect())
    }

    fn comparable(value: &str) -> Result<&str, HubError> {
        if vdp_release::changes::is_comparable_ref(value) {
            Ok(value)
        } else {
            Err(HubError::BadRequest(format!(
                "`{value}` is not a commit id or origin/<branch>"
            )))
        }
    }

    /// The files between two states (a preview's origin head and its result).
    pub fn changes(&self, from: &str, to: &str) -> Result<FileChanges, HubError> {
        let git = self.require_clone()?;
        Ok(file_changes(
            &git,
            Self::comparable(from)?,
            Self::comparable(to)?,
            None,
        )?)
    }

    /// One file's change; a zip entry by entry, or one entry's diff with `entry`.
    pub fn change_file(
        &self,
        from: &str,
        to: &str,
        path: &str,
        entry: Option<&str>,
    ) -> Result<FileDiff, HubError> {
        let git = self.require_clone()?;
        if path.is_empty() {
            return Err(HubError::BadRequest("path is required".into()));
        }
        let (from, to) = (Self::comparable(from)?, Self::comparable(to)?);
        let limit = 256 * 1024;
        match entry {
            Some(entry) => vs_changes::entry_change(&git, from, to, path, entry, limit),
            None => vs_changes::file_change(&git, from, to, path, limit),
        }
        .map_err(hub_error)
    }

    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.list()
    }

    pub fn job(&self, id: &str, after: u32) -> Result<Job, HubError> {
        self.jobs.get(id, after)
    }

    pub fn resolve(&self, id: &str, resolution: Resolution) -> Result<(), HubError> {
        self.jobs.resolve(id, resolution)
    }

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
            .map_err(|e| HubError::BadRequest(e.to_string()))
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
            result.map_err(|e| e.to_string())
        })
    }

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
                let outcome =
                    flows::build_release(git, &hub.rules, &request, expect.as_ref(), control)?;
                Ok(VsJobResult::Release { outcome }.into_result())
            },
        )
    }

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
                let known = status::status(git, &hub.rules, &hub.zips, &hub.tickets, None)?;
                let deployed_below = |zip: &str, lower: &str| {
                    known
                        .zips
                        .iter()
                        .find(|z| z.name == zip)
                        .and_then(|z| z.environments.get(lower))
                        .is_some_and(|e| e.deployed.is_some())
                };
                let outcome = flows::merge_into_environment(
                    git,
                    &hub.rules,
                    &request,
                    expect.as_ref(),
                    &deployed_below,
                    control,
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
                let outcome =
                    flows::commit_files(git, &hub.rules, &request, expect.as_ref(), control)?;
                Ok(VsJobResult::Commit { outcome }.into_result())
            },
        )
    }

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
                let outcome =
                    flows::reset_branch(git, &hub.rules, &request, expect.as_ref(), control)?;
                Ok(VsJobResult::Reset { outcome }.into_result())
            },
        )
    }
}
