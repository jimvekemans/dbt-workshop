//! The VaultSpeed side of the hub: a clone of the vaultspeed-automation-framework repository with its own jobs.
//!
//! Every flow is `vdp_vaultspeed_release`'s (the CLI runs the same code); this module adds the clone's lifecycle
//! (clone, sync, repair, re-clone), runs flows as jobs (with conflicts put to the release manager), caches the
//! status, and serves files and history for the editors. After every job the clone goes back to a detached HEAD
//! at `main` without local branches.

mod jobs;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use vdp_git::{GitCli, Vcs};
use vdp_jira::IssueTracker;
use vdp_release::changes::{ChangesRequest, FileChanges, FileDiff, file_changes};
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed_release::changes::{self as vs_changes, EntryDiffRequest, FileDiffRequest};
use vdp_vaultspeed_release::status::{self, CommitRef, Status, StatusInputs, TicketCache};
use vdp_vaultspeed_release::zips::ZipCache;
use vdp_vaultspeed_release::{VsError, VsReleaseSettings, VsRules};

use vdp_release_hub::api::{CloneRequest, RepoStatus};
use vdp_release_hub::jobs::{Job, JobLog, Jobs};
use vdp_release_hub::{Actor, CloneConfig, CloneManager, GitLab, HubError};

use crate::api::{VsFile, VsSummary};

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

/// The VaultSpeed side of the release hub: the clone of the VaultSpeed repository, its jobs and the release flows.
pub struct VsHub {
    pub(super) config: VsConfig,
    pub(super) rules: VsRules,
    clone: CloneManager,
    pub(super) jobs: Arc<Jobs>,
    tracker: Option<Arc<dyn IssueTracker>>,
    pub(super) zips: ZipCache,
    pub(super) tickets: TicketCache,
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

pub(super) fn hub_error(error: VsError) -> HubError {
    match error {
        VsError::Stale(m) => HubError::Conflict(m),
        VsError::Refused(m) | VsError::Invalid(m) => HubError::BadRequest(m),
        other => HubError::Internal(vdp_vaultspeed_domain::display_chain(&other)),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl VsHub {
    /// Creates the hub; `tracker` gives ticket details when there is one.
    pub fn new(config: VsConfig, tracker: Option<Arc<dyn IssueTracker>>) -> Arc<Self> {
        let jobs = Arc::new(Jobs::new(config.keep_jobs, config.state_dir.clone()));
        let rules = VsRules::new(config.release.clone());
        let clone = CloneManager::new(
            CloneConfig {
                repo_dir: config.repo_dir.clone(),
                repo_url: config.repo_url.clone(),
                guard: rules.guard(),
                git_timeout: config.git_timeout,
                detach_target: format!("origin/{}", config.release.main),
                protected_branch: String::new(),
                inject_clone_identity: false,
                not_cloned_error: "the VaultSpeed repository is not cloned yet".into(),
                already_cloned_error:
                    "the VaultSpeed repository is already cloned: sync it instead".into(),
            },
            Arc::clone(&jobs),
        );
        Arc::new(Self {
            jobs,
            rules,
            clone,
            config,
            tracker,
            zips: ZipCache::default(),
            tickets: TicketCache::default(),
            status: Mutex::new(None),
        })
    }

    /// The configuration.
    pub fn config(&self) -> &VsConfig {
        &self.config
    }

    /// The branch rules derived from the settings.
    pub fn rules(&self) -> &VsRules {
        &self.rules
    }

    pub(super) fn git(&self) -> GitCli {
        self.clone.git()
    }

    pub(super) fn git_as(&self, actor: &Actor) -> GitCli {
        self.clone.git_as(actor)
    }

    pub(super) fn require_clone(&self) -> Result<GitCli, HubError> {
        self.clone.require_clone()
    }

    /// The state of the clone.
    pub fn repo_status(&self) -> RepoStatus {
        self.clone.repo_status()
    }

    /// The state of the clone, the running job and the branch naming the UI needs.
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

    /// Starts a job that clones the VaultSpeed repository.
    ///
    /// # Errors
    ///
    /// As [`CloneManager::clone_repo`].
    pub fn clone_repo(&self, request: &CloneRequest) -> Result<Job, HubError> {
        self.clone.clone_repo(request)
    }

    /// Starts a job that deletes the clone (local only) and clones again.
    ///
    /// # Errors
    ///
    /// As [`CloneManager::reclone`].
    pub fn reclone(&self, request: &CloneRequest) -> Result<Job, HubError> {
        self.clone.reclone(request)
    }

    /// Starts a job that fetches origin and forgets the cached tickets.
    ///
    /// # Errors
    ///
    /// As [`CloneManager::sync`].
    pub fn sync(self: &Arc<Self>) -> Result<Job, HubError> {
        let hub = Arc::clone(self);
        self.clone.sync(move || hub.tickets.clear())
    }

    /// Starts a job that repairs the clone.
    ///
    /// # Errors
    ///
    /// As [`CloneManager::repair`].
    pub fn repair(&self) -> Result<Job, HubError> {
        self.clone.repair()
    }

    /// After a job: back to a detached HEAD, unless something was left in progress (health shows it).
    pub(super) fn tidy(&self, git: &GitCli, log: &JobLog) {
        if self.clone.tidy(git, log) {
            *lock(&self.status) = None;
        }
    }

    // --- reading ----------------------------------------------------------------------------------------

    /// Zips per environment, release branches and tickets, from origin as last fetched (cached per refs).
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Internal`] when git or a zip cannot be read; [`HubError::BadRequest`] when a rule refuses. With
    /// `refresh_jira` the cached Jira answers are dropped first.
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
            StatusInputs {
                rules: &self.rules,
                zips: &self.zips,
                tickets: &self.tickets,
                tracker: self.tracker.as_deref(),
            },
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
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when the branch is not one shown here (`main`, an environment, a release or zip branch);
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::NotFound`] when origin does not have the branch; [`HubError::Internal`] when git fails.
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
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when the branch is not one shown here; [`HubError::Conflict`] when there is no clone yet; [`HubError::Internal`] when git fails.
    pub fn tree(&self, branch: &str, dir: &str) -> Result<Vec<String>, HubError> {
        self.readable(branch)?;
        Ok(self
            .require_clone()?
            .files_at(&format!("origin/{branch}"), dir)?)
    }

    /// A branch's own recent history (for the reset picker).
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when the branch is not one shown here; [`HubError::Conflict`] when there is no clone yet; [`HubError::Internal`] when git fails.
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
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::BadRequest`] when `from` or `to` is not a commit id or `origin/<branch>`;
    /// [`HubError::Internal`] when git or a zip cannot be read.
    pub fn changes(&self, from: &str, to: &str) -> Result<FileChanges, HubError> {
        let git = self.require_clone()?;
        Ok(file_changes(
            &git,
            &ChangesRequest {
                from: Self::comparable(from)?,
                to: Self::comparable(to)?,
                history: None,
            },
        )?)
    }

    /// One file's change; a zip entry by entry, or one entry's diff with `request.entry`.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::BadRequest`] when the path is empty or `from` or `to` is not a commit id or `origin/<branch>`;
    /// [`HubError::Internal`] when git or a zip cannot be read.
    pub fn change_file(&self, request: &ChangeFileRequest<'_>) -> Result<FileDiff, HubError> {
        let git = self.require_clone()?;
        if request.path.is_empty() {
            return Err(HubError::BadRequest("path is required".into()));
        }
        let (from, to) = (
            Self::comparable(request.from)?,
            Self::comparable(request.to)?,
        );
        let limit = 256 * 1024;
        match request.entry {
            Some(entry) => vs_changes::entry_change(
                &git,
                &EntryDiffRequest {
                    from,
                    to,
                    path: request.path,
                    entry,
                    limit,
                },
            ),
            None => vs_changes::file_change(
                &git,
                &FileDiffRequest {
                    from,
                    to,
                    path: request.path,
                    limit,
                },
            ),
        }
        .map_err(hub_error)
    }
}

/// Request parameters for [`VsHub::change_file`]: the file `path` between `from` and `to`; `entry` selects one zip
/// entry's diff.
#[derive(Debug, Clone, Copy)]
pub struct ChangeFileRequest<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub path: &'a str,
    pub entry: Option<&'a str>,
}
