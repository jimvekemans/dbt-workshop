//! The hub: one clone, its analyses and the jobs that change it. The HTTP layer ([`crate::http`]) only maps
//! routes to these methods.

mod analyses;
mod announcements;
mod host;
mod releases;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard};

use vdp_git::{GitCli, Identity, PushTarget, Vcs};
use vdp_jira::IssueTracker;
use vdp_notify::Mailer;
use vdp_release::changes::{
    ChangesRequest, FileChanges, FileDiff, FileDiffRequest, file_changes, file_diff,
};
use vdp_release::{
    Analysis, BranchRules, Detection, ReleaseBranch, ReleaseSettings, Resolution, detect,
    release_branches, remote_ref,
};

use self::host::Host;
use crate::api::{
    CloneRequest, ConflictVersionsView, Defaults, ExtensionSummary, GitIdentity, Readiness,
    RepoStatus, Status,
};
use crate::auth::{GitLab, SessionUser, Sessions};
use crate::clone_manager::{CloneConfig, CloneManager};
use crate::error::HubError;
use crate::extension::{ExtensionHost, HubExtension};
use crate::jobs::{Job, JobLog, Jobs};

/// Runs dbt validation in the clone before a release is pushed; `Err` holds the reason.
pub type Validator = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// What the hub is configured with.
#[derive(Debug, Clone)]
pub struct HubConfig {
    pub repo_dir: PathBuf,
    pub repo_url: String,
    pub release: ReleaseSettings,
    pub jira_base_url: String,
    pub excluded_fix_versions: Vec<String>,
    pub default_recipients: Vec<String>,
    pub keep_jobs: usize,
    /// Stops any git command running longer (`None`: no limit).
    pub git_timeout: Option<std::time::Duration>,
    /// A conflict without a decision for this long aborts the run.
    pub conflict_timeout: std::time::Duration,
    /// Analyses and jobs are kept here across restarts (`None`: in memory only).
    pub state_dir: Option<PathBuf>,
    /// Sign-in with GitLab tokens (`None`: no sign-in; commits use the clone's identity, pushes go to origin).
    pub gitlab: Option<GitLab>,
}

/// Who a request acts as: the commit identity and where pushes go.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    pub identity: Option<Identity>,
    pub push: PushTarget,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Stored {
    pub(super) created: i64,
    pub(super) branch: String,
    pub(super) analysis: Analysis,
    pub(super) selection: BTreeSet<String>,
    #[serde(default)]
    pub(super) version: u32,
}

/// What survives a restart (in `[release_hub].state_dir`).
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct SavedAnalyses {
    next: u64,
    analyses: BTreeMap<u64, Stored>,
}

/// Writes `value` as JSON atomically (a temporary file renamed over the old one).
pub(crate) fn save_json<T: serde::Serialize>(dir: &Path, name: &str, value: &T) {
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let temporary = dir.join(format!(".{name}.tmp"));
        std::fs::write(
            &temporary,
            serde_json::to_vec(value).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&temporary, dir.join(name))
    };
    if let Err(error) = write() {
        tracing::warn!("could not save {name} in {}: {error}", dir.display());
    }
}

pub(crate) fn load_json<T: serde::de::DeserializeOwned>(dir: &Path, name: &str) -> Option<T> {
    let bytes = std::fs::read(dir.join(name)).ok()?;
    serde_json::from_slice(&bytes)
        .map_err(|e| tracing::warn!("ignoring {name}: {e}"))
        .ok()
}

/// The release hub.
pub struct Hub {
    pub(super) config: HubConfig,
    /// The source branch (dev) is never written; only release branches and main are.
    pub(super) rules: BranchRules,
    clone: CloneManager,
    /// `Err`: why Jira is not available (tickets then carry a `jira_error`).
    pub(super) tracker: Result<Arc<dyn IssueTracker>, String>,
    pub(super) mailer: Arc<dyn Mailer>,
    pub(super) validator: Option<Validator>,
    pub(super) jobs: Arc<Jobs>,
    pub(super) sessions: Sessions,
    pub(super) analyses: Mutex<BTreeMap<u64, Stored>>,
    pub(super) next_analysis: AtomicU64,
    extensions: Vec<Arc<dyn HubExtension>>,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub")
            .field("config", &self.config)
            .field("tracker", &self.tracker.as_ref().map(|_| "<jira>"))
            .field("mailer", &self.mailer)
            .finish_non_exhaustive()
    }
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) fn analysis_id(n: u64) -> String {
    format!("a{n}")
}

pub(super) fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Collaborators wired into [`Hub::new`].
pub struct HubServices {
    /// `Err`: why Jira is not available (tickets then carry a `jira_error`).
    pub tracker: Result<Arc<dyn IssueTracker>, String>,
    pub mailer: Arc<dyn Mailer>,
    pub validator: Option<Validator>,
    pub extensions: Vec<Arc<dyn HubExtension>>,
}

impl std::fmt::Debug for HubServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubServices")
            .field("tracker", &self.tracker.as_ref().map(|_| "<jira>"))
            .field("mailer", &self.mailer)
            .field("validator", &self.validator.as_ref().map(|_| "<validator>"))
            .field("extensions", &self.extensions.len())
            .finish()
    }
}

impl Hub {
    /// Creates the hub from its configuration and services.
    pub fn new(config: HubConfig, services: HubServices) -> Arc<Self> {
        let HubServices {
            tracker,
            mailer,
            validator,
            extensions,
        } = services;
        let saved: SavedAnalyses = config
            .state_dir
            .as_deref()
            .and_then(|dir| load_json(dir, "analyses.json"))
            .unwrap_or_default();
        let rules = BranchRules::from_settings(&config.release);
        let jobs = Arc::new(Jobs::new(config.keep_jobs, config.state_dir.clone()));
        let clone = CloneManager::new(
            CloneConfig {
                repo_dir: config.repo_dir.clone(),
                repo_url: config.repo_url.clone(),
                guard: rules.guard(),
                git_timeout: config.git_timeout,
                detach_target: remote_ref(&rules.source),
                protected_branch: rules.source.clone(),
                inject_clone_identity: true,
                not_cloned_error: format!(
                    "{} is not cloned yet: clone the repository first",
                    config.repo_dir.display()
                ),
                already_cloned_error: "the repository is already cloned: sync it instead".into(),
            },
            Arc::clone(&jobs),
        );
        Arc::new(Self {
            extensions,
            jobs,
            sessions: Sessions::default(),
            rules,
            clone,
            config,
            tracker,
            mailer,
            validator,
            next_analysis: AtomicU64::new(saved.next),
            analyses: Mutex::new(saved.analyses),
        })
    }

    /// The hub's configuration.
    pub fn config(&self) -> &HubConfig {
        &self.config
    }

    pub(super) fn save_analyses(&self, analyses: &BTreeMap<u64, Stored>) {
        if let Some(dir) = &self.config.state_dir {
            #[derive(serde::Serialize)]
            struct Saving<'a> {
                next: u64,
                analyses: &'a BTreeMap<u64, Stored>,
            }
            save_json(
                dir,
                "analyses.json",
                &Saving {
                    next: self
                        .next_analysis
                        .load(std::sync::atomic::Ordering::Relaxed),
                    analyses,
                },
            );
        }
    }

    pub(super) fn git(&self) -> GitCli {
        self.clone.git()
    }

    /// A client that commits as `actor` and pushes where they may.
    pub(super) fn git_as(&self, actor: &Actor) -> GitCli {
        self.clone.git_as(actor)
    }

    /// Who a request acts as. With GitLab sign-in: the session's person (`write`: who may release); without:
    /// the clone's configured identity.
    ///
    /// # Errors
    ///
    /// [`HubError::Unauthorized`] when sign-in is on and `session` is missing or expired; [`HubError::Forbidden`] when `write`
    /// is set and the person may not release; [`HubError::Internal`] when, without sign-in, the clone's identity cannot be
    /// read.
    pub fn actor(&self, session: Option<&str>, write: bool) -> Result<Actor, HubError> {
        let Some(gitlab) = &self.config.gitlab else {
            return Ok(Actor {
                identity: self.identity()?.map(|i| Identity {
                    name: i.name,
                    email: i.email,
                }),
                push: PushTarget::Origin,
            });
        };
        let session = session
            .and_then(|id| self.sessions.get(id))
            .ok_or_else(|| HubError::Unauthorized("sign in with your GitLab token".into()))?;
        if write && !session.user.can_release {
            return Err(HubError::Forbidden(format!(
                "you may not release: {}",
                session.user.reasons.join("; ")
            )));
        }
        Ok(Actor {
            identity: Some(session.identity()),
            push: session.push_target(gitlab),
        })
    }

    /// Signs in with a GitLab token; returns the session id and the person with their rights.
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when sign-in is off or the token is empty; otherwise as [`GitLab::user`]: [`HubError::Unauthorized`] when GitLab refuses the token; [`HubError::Forbidden`] when the token has no access to the
    /// project; [`HubError::Internal`] when GitLab is unreachable or its answer cannot be read.
    pub fn sign_in(&self, token: &str) -> Result<(String, SessionUser), HubError> {
        let Some(gitlab) = &self.config.gitlab else {
            return Err(HubError::BadRequest("sign-in is off in this hub".into()));
        };
        let token = token.trim();
        if token.is_empty() {
            return Err(HubError::BadRequest("a token is required".into()));
        }
        let mut user = gitlab.sign_in(token, &self.rules.main, &self.rules.prefix)?;
        for extension in &self.extensions {
            user.rights.insert(
                extension.segment().to_owned(),
                extension.rights(gitlab, token),
            );
        }
        let id = self.sessions.create(user.clone(), token.to_owned());
        tracing::info!(user = %user.username, can_release = user.can_release, "signed in");
        Ok((id, user))
    }

    /// The extension served below `/api/v1/<segment>`, if one is registered.
    pub fn extension(&self, segment: &str) -> Option<&Arc<dyn HubExtension>> {
        self.extensions.iter().find(|e| e.segment() == segment)
    }

    /// What `extension` may ask of the hub while it handles a request.
    pub(crate) fn host_of<'a>(
        &'a self,
        extension: &'a dyn HubExtension,
    ) -> impl ExtensionHost + 'a {
        Host {
            hub: self,
            segment: extension.segment(),
        }
    }

    /// The person of `session`, `None` when it is missing, unknown or expired.
    pub fn session_user(&self, session: Option<&str>) -> Option<SessionUser> {
        session.and_then(|id| self.sessions.get(id)).map(|s| s.user)
    }

    /// Ends `session`, if there is one.
    pub fn sign_out(&self, session: Option<&str>) {
        if let Some(id) = session {
            self.sessions.remove(id);
        }
    }

    pub(super) fn require_clone(&self) -> Result<GitCli, HubError> {
        self.clone.require_clone()
    }

    /// The configured identity of the clone (`user.name`/`user.email`).
    fn identity(&self) -> Result<Option<GitIdentity>, HubError> {
        self.clone.identity()
    }

    pub(super) fn require_identity(actor: &Actor) -> Result<(), HubError> {
        match &actor.identity {
            Some(i) if i.is_complete() => Ok(()),
            _ => Err(HubError::Conflict(
                "set the git identity (name and email) first: commits are made in its name".into(),
            )),
        }
    }

    // ---- status ----

    /// The state of the clone.
    pub fn repo_status(&self) -> RepoStatus {
        self.clone.repo_status()
    }

    /// Aborts what is in progress, removes a stale lock, discards local changes and branches, detaches HEAD at
    /// the source. Never touches origin.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running.
    pub fn repair(&self) -> Result<Job, HubError> {
        self.clone.repair()
    }

    /// Deletes the clone (local only) and clones again.
    ///
    /// # Errors
    ///
    /// [`HubError::Busy`] when another job is running.
    pub fn reclone(&self, request: &CloneRequest) -> Result<Job, HubError> {
        self.clone.reclone(request)
    }

    /// The hub's status for the UI; `session` is the caller's session id, if any.
    pub fn status(&self, session: Option<&str>) -> Status {
        let release = &self.config.release;
        Status {
            version: env!("CARGO_PKG_VERSION").into(),
            auth: if self.config.gitlab.is_some() {
                "gitlab".into()
            } else {
                "none".into()
            },
            user: self.session_user(session),
            repo: self.repo_status(),
            identity: self.identity().ok().flatten(),
            jira: Readiness::from_result(self.tracker.as_ref().map(|_| ()).map_err(Clone::clone)),
            smtp: Readiness::from_result(self.mailer.readiness()),
            active_job: self.jobs.active(),
            defaults: Defaults {
                repo_url: self.config.repo_url.clone(),
                source: release.from.clone(),
                main: release.to.clone(),
                branch_prefix: release.branch_prefix.clone(),
                ticket_prefix: release.ticket_prefix.clone(),
                recipients: self.config.default_recipients.clone(),
            },
            extensions: self
                .extensions
                .iter()
                .map(|e| (e.segment().to_owned(), ExtensionSummary(e.summary())))
                .collect(),
        }
    }

    /// Stores the identity in the clone's git config (as the Python UI did).
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when sign-in is on (commits then use the GitLab account) or the name or email is blank;
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Internal`] when a git operation fails.
    pub fn set_identity(&self, identity: &GitIdentity) -> Result<GitIdentity, HubError> {
        if self.config.gitlab.is_some() {
            return Err(HubError::BadRequest(
                "commits use your GitLab account's name and email in this hub".into(),
            ));
        }
        let identity = GitIdentity {
            name: identity.name.trim().to_owned(),
            email: identity.email.trim().to_owned(),
        };
        let git_identity = Identity {
            name: identity.name.clone(),
            email: identity.email.clone(),
        };
        if !git_identity.is_complete() {
            return Err(HubError::BadRequest(
                "a name and a valid email are required".into(),
            ));
        }
        self.require_clone()?.set_identity(&git_identity)?;
        Ok(identity)
    }

    // ---- jobs ----

    /// The job `id` with the steps after `after`.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such job.
    pub fn job(&self, id: &str, after: u32) -> Result<Job, HubError> {
        self.jobs.get(id, after)
    }

    /// The release manager's decision on a waiting job's conflict.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when `actor` has no complete git identity or the job is not waiting for a decision;
    /// [`HubError::NotFound`] when there is no such job; [`HubError::BadRequest`] when `resolution` names a file that is not
    /// in the conflict.
    pub fn resolve(&self, id: &str, resolution: Resolution, actor: &Actor) -> Result<(), HubError> {
        Self::require_identity(actor)?;
        self.jobs.resolve(id, resolution)
    }

    /// The three versions of a file in a waiting job's conflict.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no such job or it is not waiting for a decision; [`HubError::BadRequest`] when
    /// `path` is not a file of the conflict; [`HubError::Internal`] when a git operation fails.
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

    /// The jobs the hub knows, running and finished.
    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.list()
    }

    /// Starts a job that clones the repository.
    ///
    /// # Errors
    ///
    /// As [`CloneManager::clone_repo`].
    pub fn clone_repo(&self, request: &CloneRequest) -> Result<Job, HubError> {
        self.clone.clone_repo(request)
    }

    pub(super) fn tidy(&self, git: &GitCli, log: &JobLog) {
        self.clone.tidy(git, log);
    }

    /// Starts a job that fetches origin.
    ///
    /// # Errors
    ///
    /// As [`CloneManager::sync`].
    pub fn sync(&self) -> Result<Job, HubError> {
        self.clone.sync(|| {})
    }

    // ---- branches ----

    /// The release branches on origin, newest first.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Internal`] when a git operation fails.
    pub fn release_branches(&self) -> Result<Vec<ReleaseBranch>, HubError> {
        let git = self.require_clone()?;
        let release = &self.config.release;
        Ok(release_branches(&git, &release.branch_prefix, &release.to)?)
    }

    /// What promoting `branch` to main would do (new release or patch, and why).
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::BadRequest`] when `branch` is not a release
    /// branch; [`HubError::NotFound`] when origin does not have it; [`HubError::Internal`] when a git operation fails.
    pub fn promotion(&self, branch: &str) -> Result<Detection, HubError> {
        let git = self.require_clone()?;
        let branch = self.rules.release_target(branch)?;
        git.rev_parse(&remote_ref(&branch))
            .map_err(|_| HubError::NotFound(format!("branch {branch} on origin")))?;
        Ok(detect(&git, &branch, &self.rules)?)
    }

    // ---- what a deploy changes ----

    fn comparable(value: &str) -> Result<&str, HubError> {
        if vdp_release::changes::is_comparable_ref(value) {
            Ok(value)
        } else {
            Err(HubError::BadRequest(format!(
                "`{value}` is not a commit id or origin/<branch>"
            )))
        }
    }

    /// The files between two commits, with the commits (and authors) that touched each.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::BadRequest`] when `from` or `to` is not a commit id or `origin/<branch>`; [`HubError::Internal`] when a git operation fails.
    pub fn changes(&self, from: &str, to: &str) -> Result<FileChanges, HubError> {
        let git = self.require_clone()?;
        let (from, to) = (Self::comparable(from)?, Self::comparable(to)?);
        Ok(file_changes(
            &git,
            &ChangesRequest {
                from,
                to,
                history: Some((from, to)),
            },
        )?)
    }

    /// What promoting `branch` changes on main: for a new release main becomes the branch; for a patch, the
    /// result of merging it (`git merge-tree`, nothing touched), with the paths that would conflict.
    ///
    /// # Errors
    ///
    /// As [`Hub::promotion`], and [`HubError::Internal`] when a git operation fails while comparing main with the branch.
    pub fn promotion_changes(&self, branch: &str) -> Result<FileChanges, HubError> {
        let detection = self.promotion(branch)?;
        let git = self.require_clone()?;
        let branch = self.rules.release_target(branch)?;
        let main = git.rev_parse(&remote_ref(&self.rules.main))?;
        let head = git.rev_parse(&remote_ref(&branch))?;
        if detection.nothing_to_promote {
            return Ok(file_changes(
                &git,
                &ChangesRequest {
                    from: &main,
                    to: &main,
                    history: None,
                },
            )?);
        }
        let (to, conflicts) = match detection.kind {
            vdp_release::PromotionKind::New => (head.clone(), Vec::new()),
            vdp_release::PromotionKind::Patch => {
                let merged = git.merge_tree(&main, &head)?;
                (merged.tree, merged.conflicts)
            }
        };
        let mut changes = file_changes(
            &git,
            &ChangesRequest {
                from: &main,
                to: &to,
                history: Some((&main, &head)),
            },
        )?;
        changes.conflicts = conflicts;
        Ok(changes)
    }

    /// One file's diff between two commits or trees.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::BadRequest`] when `path` is empty or `from` or `to` is not a commit id or `origin/<branch>`;
    /// [`HubError::Internal`] when a git operation fails.
    pub fn change_file(&self, from: &str, to: &str, path: &str) -> Result<FileDiff, HubError> {
        let git = self.require_clone()?;
        if path.is_empty() {
            return Err(HubError::BadRequest("path is required".into()));
        }
        Ok(file_diff(
            &git,
            &FileDiffRequest {
                from: Self::comparable(from)?,
                to: Self::comparable(to)?,
                path,
                limit: 256 * 1024,
            },
        )?)
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::Duration;

    use vdp_notify::{Announcement, Delivery, NotifyError};

    use super::*;
    use crate::jobs::JobStatus;

    #[derive(Debug)]
    struct DummyMailer;

    impl Mailer for DummyMailer {
        fn readiness(&self) -> Result<(), String> {
            Ok(())
        }

        fn send(&self, _: &Announcement, _: &[String]) -> Result<Delivery, NotifyError> {
            unreachable!("test sends nothing")
        }
    }

    fn run_git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Seed", "-c", "user.email=seed@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[test]
    fn hub_git_injects_clone_identity_into_commits() {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let seed = dir.path().join("seed");
        let clone_dir = dir.path().join("clone");
        assert!(
            Command::new("git")
                .args(["init", "--bare"])
                .arg(&origin)
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(
            Command::new("git")
                .args(["clone", origin.to_str().unwrap(), seed.to_str().unwrap()])
                .output()
                .unwrap()
                .status
                .success()
        );
        run_git(&seed, &["checkout", "-b", "dev"]);
        std::fs::write(seed.join("README.md"), "seed\n").unwrap();
        run_git(&seed, &["add", "README.md"]);
        run_git(&seed, &["commit", "-m", "seed dev"]);
        run_git(&seed, &["push", "-u", "origin", "dev"]);

        let hub = Hub::new(
            HubConfig {
                repo_dir: clone_dir.clone(),
                repo_url: origin.to_string_lossy().into_owned(),
                release: ReleaseSettings::default(),
                jira_base_url: String::new(),
                excluded_fix_versions: Vec::new(),
                default_recipients: Vec::new(),
                keep_jobs: 10,
                git_timeout: None,
                conflict_timeout: Duration::from_secs(1),
                state_dir: None,
                gitlab: None,
            },
            HubServices {
                tracker: Err("jira off".into()),
                mailer: Arc::new(DummyMailer),
                validator: None,
                extensions: Vec::new(),
            },
        );

        let clone_job = hub.clone_repo(&CloneRequest { url: None }).unwrap();
        for _ in 0..500 {
            let current = hub.job(&clone_job.id, 0).unwrap();
            if current.status != JobStatus::Running {
                assert_eq!(current.status, JobStatus::Done, "{:?}", current.error);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        hub.set_identity(&GitIdentity {
            name: "Release Manager".into(),
            email: "rm@example.com".into(),
        })
        .unwrap();

        let git = hub.git();
        // Overwrite .git/config after hub.git() is built so commits only get Release Manager if CloneManager injected it.
        run_git(
            &clone_dir,
            &["config", "--local", "user.name", "Fallback User"],
        );
        run_git(
            &clone_dir,
            &["config", "--local", "user.email", "fallback@example.com"],
        );

        git.checkout_reset("release/2026-Q1", "origin/dev").unwrap();
        git.commit_files(
            &[("note.txt".into(), "hello\n".into())],
            "test injected identity",
        )
        .unwrap()
        .expect("commit created");

        let who = run_git(&clone_dir, &["log", "-1", "--format=%an|%ae|%cn|%ce"]);
        assert_eq!(
            who,
            "Release Manager|rm@example.com|Release Manager|rm@example.com"
        );
    }
}
