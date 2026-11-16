//! The hub: one clone, its analyses and the jobs that change it. The HTTP layer ([`crate::http`]) only maps
//! routes to these methods.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use vdp_git::{GitCli, Identity, PushTarget, Vcs};
use vdp_jira::{FixVersion, IssueTracker};
use vdp_notify::{Announcement, Mailer};
use vdp_release::changes::{FileChanges, FileDiff, file_changes, file_diff};
use vdp_release::{
    Analyser, Analysis, AnalysisKind, AnalysisRequest, BranchRules, ConflictReport, Conflicts,
    Detection, PatchFlow, Progress, PromoteOptions, Promotion, ReleaseBranch, ReleaseFlow,
    ReleaseOptions, ReleaseSettings, Resolution, ScopeAction, apply_options, check_scope, detect,
    local_name, release_branches, remote_ref,
};

use crate::api::{
    AnalysisCreate, AnalysisSummary, AnalysisView, AnnouncementSend, AnnouncementSent,
    CloneRequest, ConflictVersionsView, Defaults, GitIdentity, HealthProblem, PatchCreate,
    PromotionCreate, Readiness, ReleaseCreate, RepoStatus, SelectionUpdate, Status,
};
use crate::auth::{GitLab, SessionUser, Sessions};
use crate::error::HubError;
use crate::jobs::{Job, JobKind, JobLog, JobResult, Jobs, now_ms};

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
    /// The VaultSpeed side (`None`: dbt releases only).
    pub vaultspeed: Option<crate::vaultspeed::VsConfig>,
}

/// Who a request acts as: the commit identity and where pushes go.
#[derive(Debug, Clone, Default)]
pub struct Actor {
    pub identity: Option<Identity>,
    pub push: PushTarget,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Stored {
    created: i64,
    branch: String,
    analysis: Analysis,
    selection: BTreeSet<String>,
    #[serde(default)]
    version: u32,
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
    config: HubConfig,
    /// The source branch (dev) is never written; only release branches and main are.
    rules: BranchRules,
    /// `Err`: why Jira is not available (tickets then carry a `jira_error`).
    tracker: Result<Arc<dyn IssueTracker>, String>,
    mailer: Arc<dyn Mailer>,
    validator: Option<Validator>,
    jobs: Arc<Jobs>,
    sessions: Sessions,
    analyses: Mutex<BTreeMap<u64, Stored>>,
    next_analysis: AtomicU64,
    vaultspeed: Option<Arc<crate::vaultspeed::VsHub>>,
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn analysis_id(n: u64) -> String {
    format!("a{n}")
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl Hub {
    pub fn new(
        config: HubConfig,
        tracker: Result<Arc<dyn IssueTracker>, String>,
        mailer: Arc<dyn Mailer>,
        validator: Option<Validator>,
    ) -> Arc<Self> {
        let saved: SavedAnalyses = config
            .state_dir
            .as_deref()
            .and_then(|dir| load_json(dir, "analyses.json"))
            .unwrap_or_default();
        let vaultspeed = config
            .vaultspeed
            .clone()
            .map(|vs| crate::vaultspeed::VsHub::new(vs, tracker.as_ref().ok().cloned()));
        Arc::new(Self {
            vaultspeed,
            jobs: Arc::new(Jobs::new(config.keep_jobs, config.state_dir.clone())),
            sessions: Sessions::default(),
            rules: BranchRules::from_settings(&config.release),
            config,
            tracker,
            mailer,
            validator,
            next_analysis: AtomicU64::new(saved.next),
            analyses: Mutex::new(saved.analyses),
        })
    }

    pub fn config(&self) -> &HubConfig {
        &self.config
    }

    fn save_analyses(&self, analyses: &BTreeMap<u64, Stored>) {
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
                    next: self.next_analysis.load(Ordering::Relaxed),
                    analyses,
                },
            );
        }
    }

    fn git(&self) -> GitCli {
        let identity = self.identity().ok().flatten().map(|i| Identity {
            name: i.name,
            email: i.email,
        });
        GitCli::new(&self.config.repo_dir)
            .with_identity(identity)
            .with_guard(self.rules.guard())
            .with_timeout(self.config.git_timeout)
    }

    /// A client that commits as `actor` and pushes where they may.
    fn git_as(&self, actor: &Actor) -> GitCli {
        GitCli::new(&self.config.repo_dir)
            .with_identity(actor.identity.clone())
            .with_guard(self.rules.guard())
            .with_timeout(self.config.git_timeout)
            .with_push_target(actor.push.clone())
    }

    /// Who a request acts as. With GitLab sign-in: the session's person (`write`: who may release); without:
    /// the clone's configured identity.
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

    pub fn sign_in(&self, token: &str) -> Result<(String, SessionUser), HubError> {
        let Some(gitlab) = &self.config.gitlab else {
            return Err(HubError::BadRequest("sign-in is off in this hub".into()));
        };
        let token = token.trim();
        if token.is_empty() {
            return Err(HubError::BadRequest("a token is required".into()));
        }
        let mut user = gitlab.sign_in(token, &self.rules.main, &self.rules.prefix)?;
        if let Some(vs) = &self.vaultspeed {
            user.vaultspeed = Some(Self::vaultspeed_rights(vs, gitlab, token));
        }
        let id = self.sessions.create(user.clone(), token.to_owned());
        tracing::info!(user = %user.username, can_release = user.can_release, "signed in");
        Ok((id, user))
    }

    /// Per branch of the framework repository; a GitLab error refuses everything (with the reason).
    fn vaultspeed_rights(
        vs: &crate::vaultspeed::VsHub,
        dbt: &GitLab,
        token: &str,
    ) -> Vec<crate::auth::BranchRight> {
        let samples = vs.rights_samples();
        let gitlab = vs.config().gitlab.clone().unwrap_or_else(|| dbt.clone());
        let rights = gitlab
            .user(token)
            .and_then(|user| gitlab.rights(token, &user, &samples));
        match rights {
            Ok(rights) => rights,
            Err(error) => samples
                .into_iter()
                .map(|(branch, _)| crate::auth::BranchRight {
                    branch,
                    push: false,
                    force: false,
                    reason: Some(format!("{}: {error}", gitlab.project)),
                })
                .collect(),
        }
    }

    /// The VaultSpeed side, or 404 when this hub does not steer it.
    pub fn vaultspeed(&self) -> Result<&Arc<crate::vaultspeed::VsHub>, HubError> {
        self.vaultspeed
            .as_ref()
            .ok_or_else(|| HubError::NotFound("VaultSpeed releases are not configured".into()))
    }

    /// Who a VaultSpeed write acts as: signed in, with the GitLab right on `branch` (push, or force push), and
    /// pushing to the framework project as themselves.
    pub fn vaultspeed_actor(
        &self,
        session: Option<&str>,
        branch: &str,
        need: crate::vaultspeed::Need,
    ) -> Result<Actor, HubError> {
        let vs = self.vaultspeed()?;
        let Some(dbt_gitlab) = &self.config.gitlab else {
            // No sign-in (local use): the dbt clone's identity, pushing to origin.
            return self.actor(session, false);
        };
        let session = session
            .and_then(|id| self.sessions.get(id))
            .ok_or_else(|| HubError::Unauthorized("sign in with your GitLab token".into()))?;
        let rights_branch = vs.rights_branch(branch);
        let right = session
            .user
            .vaultspeed_right(&rights_branch)
            .cloned()
            .ok_or_else(|| {
                HubError::Forbidden(format!("no rights known for {branch}: sign in again"))
            })?;
        let allowed = match need {
            crate::vaultspeed::Need::Push => right.push,
            crate::vaultspeed::Need::Force => right.force,
        };
        if !allowed {
            return Err(HubError::Forbidden(right.reason.unwrap_or_else(|| {
                format!(
                    "you may not {} {branch}",
                    if need == crate::vaultspeed::Need::Force {
                        "force-push"
                    } else {
                        "push to"
                    }
                )
            })));
        }
        if session.user.email.is_empty() {
            return Err(HubError::Forbidden(
                "your GitLab account has no commit email: set one, commits need it".into(),
            ));
        }
        let gitlab = vs
            .config()
            .gitlab
            .clone()
            .unwrap_or_else(|| dbt_gitlab.clone());
        Ok(Actor {
            identity: Some(session.identity()),
            push: session.push_target(&gitlab),
        })
    }

    /// The CI pipeline for a commit of the framework repository, read with the signed-in person's token.
    pub fn vaultspeed_pipeline(
        &self,
        session: Option<&str>,
        sha: &str,
    ) -> Result<Option<crate::auth::Pipeline>, HubError> {
        let vs = self.vaultspeed()?;
        let Some(dbt_gitlab) = &self.config.gitlab else {
            return Ok(None);
        };
        let session = session
            .and_then(|id| self.sessions.get(id))
            .ok_or_else(|| HubError::Unauthorized("sign in with your GitLab token".into()))?;
        let gitlab = vs
            .config()
            .gitlab
            .clone()
            .unwrap_or_else(|| dbt_gitlab.clone());
        gitlab.pipeline(session.token(), sha)
    }

    pub fn session_user(&self, session: Option<&str>) -> Option<SessionUser> {
        session.and_then(|id| self.sessions.get(id)).map(|s| s.user)
    }

    pub fn sign_out(&self, session: Option<&str>) {
        if let Some(id) = session {
            self.sessions.remove(id);
        }
    }

    fn require_clone(&self) -> Result<GitCli, HubError> {
        let git = self.git();
        if git.is_repository() {
            Ok(git)
        } else {
            Err(HubError::Conflict(format!(
                "{} is not cloned yet: clone the repository first",
                self.config.repo_dir.display()
            )))
        }
    }

    /// The configured identity of the clone (`user.name`/`user.email`).
    fn identity(&self) -> Result<Option<GitIdentity>, HubError> {
        let git = GitCli::new(&self.config.repo_dir);
        if !git.is_repository() {
            return Ok(None);
        }
        Ok(git.identity()?.map(|i| GitIdentity {
            name: i.name,
            email: i.email,
        }))
    }

    fn require_identity(actor: &Actor) -> Result<(), HubError> {
        match &actor.identity {
            Some(i) if i.is_complete() => Ok(()),
            _ => Err(HubError::Conflict(
                "set the git identity (name and email) first: commits are made in its name".into(),
            )),
        }
    }

    // ---- status ----

    pub fn repo_status(&self) -> RepoStatus {
        repo_status_of(
            &self.config.repo_dir,
            self.jobs.active().is_some(),
            &self.rules.source,
        )
    }

    /// Aborts what is in progress, removes a stale lock, discards local changes and branches, detaches HEAD at
    /// the source. Never touches origin.
    pub fn repair(self: &Arc<Self>) -> Result<Job, HubError> {
        self.require_clone()?;
        let hub = Arc::clone(self);
        self.jobs
            .start(JobKind::Repair, "repair the clone", move |log| {
                let git = GitCli::new(&hub.config.repo_dir).with_timeout(hub.config.git_timeout);
                for action in git
                    .repair(&remote_ref(&hub.rules.source))
                    .map_err(|e| e.to_string())?
                {
                    log.info(action);
                }
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

    /// Deletes the clone (local only) and clones again.
    pub fn reclone(self: &Arc<Self>, request: &CloneRequest) -> Result<Job, HubError> {
        let url = request
            .url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .or_else(|| {
                GitCli::new(&self.config.repo_dir)
                    .origin_url()
                    .ok()
                    .flatten()
            })
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
                log.info("cloned");
                hub.detach(&git, log)?;
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

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
            vaultspeed: self.vaultspeed.as_ref().map(|vs| vs.summary()),
        }
    }

    /// Stores the identity in the clone's git config (as the Python UI did).
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

    pub fn job(&self, id: &str, after: u32) -> Result<Job, HubError> {
        self.jobs.get(id, after)
    }

    /// The release manager's decision on a waiting job's conflict.
    pub fn resolve(&self, id: &str, resolution: Resolution, actor: &Actor) -> Result<(), HubError> {
        Self::require_identity(actor)?;
        self.jobs.resolve(id, resolution)
    }

    /// The three versions of a file in a waiting job's conflict.
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

    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.list()
    }

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
                "the repository is already cloned: sync it instead".into(),
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
                log.info(format!(
                    "cloning {url} into {}",
                    hub.config.repo_dir.display()
                ));
                let git = hub.git();
                git.clone_from(&url).map_err(|e| e.to_string())?;
                log.info("cloned");
                hub.detach(&git, log)?;
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

    /// HEAD detached at the source, and no local branches: there is no local `dev` a revert could land on,
    /// and every flow starts its branch from origin.
    fn detach(&self, git: &GitCli, log: &JobLog) -> Result<(), String> {
        let source = remote_ref(&self.rules.source);
        git.detach_at(&source).map_err(|e| e.to_string())?;
        let deleted = git.delete_local_branches().map_err(|e| e.to_string())?;
        log.info(format!(
            "HEAD detached at {source}; local branches removed: {}",
            if deleted.is_empty() {
                "none".to_owned()
            } else {
                deleted.join(", ")
            }
        ));
        Ok(())
    }

    /// After a job: back to a detached HEAD without local branches. A failure that left something in
    /// progress (or uncommitted) keeps its state for the health check and repair.
    fn tidy(&self, git: &GitCli, log: &JobLog) {
        let state = git.workspace_state();
        if state.operation.is_some() || state.index_lock || !state.dirty.is_empty() {
            log.warn("the clone is left as it is: check its health (repair resets it)");
            return;
        }
        if let Err(error) = self.detach(git, log) {
            log.warn(format!("could not tidy the clone: {error}"));
        }
    }

    pub fn sync(self: &Arc<Self>) -> Result<Job, HubError> {
        self.require_clone()?;
        let hub = Arc::clone(self);
        self.jobs
            .start(JobKind::Sync, "sync with origin", move |log| {
                log.info("fetching origin (all branches, pruned, tags)");
                hub.git().fetch_all().map_err(|e| e.to_string())?;
                log.info("up to date");
                Ok(JobResult::Repo {
                    repo: hub.repo_status(),
                })
            })
    }

    // ---- branches ----

    pub fn release_branches(&self) -> Result<Vec<ReleaseBranch>, HubError> {
        let git = self.require_clone()?;
        let release = &self.config.release;
        Ok(release_branches(&git, &release.branch_prefix, &release.to)?)
    }

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
    pub fn changes(&self, from: &str, to: &str) -> Result<FileChanges, HubError> {
        let git = self.require_clone()?;
        let (from, to) = (Self::comparable(from)?, Self::comparable(to)?);
        Ok(file_changes(&git, from, to, Some((from, to)))?)
    }

    /// What promoting `branch` changes on main: for a new release main becomes the branch; for a patch, the
    /// result of merging it (`git merge-tree`, nothing touched), with the paths that would conflict.
    pub fn promotion_changes(&self, branch: &str) -> Result<FileChanges, HubError> {
        let detection = self.promotion(branch)?;
        let git = self.require_clone()?;
        let branch = self.rules.release_target(branch)?;
        let main = git.rev_parse(&remote_ref(&self.rules.main))?;
        let head = git.rev_parse(&remote_ref(&branch))?;
        if detection.nothing_to_promote {
            return Ok(file_changes(&git, &main, &main, None)?);
        }
        let (to, conflicts) = match detection.kind {
            vdp_release::PromotionKind::New => (head.clone(), Vec::new()),
            vdp_release::PromotionKind::Patch => {
                let merged = git.merge_tree(&main, &head)?;
                (merged.tree, merged.conflicts)
            }
        };
        let mut changes = file_changes(&git, &main, &to, Some((&main, &head)))?;
        changes.conflicts = conflicts;
        Ok(changes)
    }

    /// One file's diff between two commits or trees.
    pub fn change_file(&self, from: &str, to: &str, path: &str) -> Result<FileDiff, HubError> {
        let git = self.require_clone()?;
        if path.is_empty() {
            return Err(HubError::BadRequest("path is required".into()));
        }
        Ok(file_diff(
            &git,
            Self::comparable(from)?,
            Self::comparable(to)?,
            path,
            256 * 1024,
        )?)
    }

    // ---- analyses ----

    fn request_for(&self, create: &AnalysisCreate) -> Result<(AnalysisRequest, String), HubError> {
        let release = &self.config.release;
        let given = |v: &Option<String>| v.clone().filter(|s| !s.trim().is_empty());
        let source = remote_ref(&given(&create.source).unwrap_or_else(|| release.from.clone()));
        let (compare, branch) = match create.kind {
            AnalysisKind::Release => (
                given(&create.compare).unwrap_or_else(|| release.to.clone()),
                None,
            ),
            AnalysisKind::Patch => {
                let branch = given(&create.compare).ok_or_else(|| {
                    HubError::BadRequest("a patch needs the release branch (compare)".into())
                })?;
                let target = self.rules.release_target(&branch)?;
                check_scope(&self.git(), &self.rules, &target, ScopeAction::Patch)?;
                (branch.clone(), Some(target))
            }
        };
        let fix_version = match given(&create.fix_version) {
            Some(v) => v,
            None => branch
                .as_deref()
                .and_then(|b| b.strip_prefix(&release.branch_prefix))
                .map(str::to_owned)
                .ok_or_else(|| HubError::BadRequest("fix_version is required".into()))?,
        };
        fix_version
            .parse::<FixVersion>()
            .map_err(|e| HubError::BadRequest(format!("fix_version: {e}")))?;
        let branch = branch.unwrap_or_else(|| format!("{}{fix_version}", release.branch_prefix));
        Ok((
            AnalysisRequest {
                kind: create.kind,
                source,
                compare: remote_ref(&compare),
                fix_version,
            },
            branch,
        ))
    }

    pub fn create_analysis(self: &Arc<Self>, create: &AnalysisCreate) -> Result<Job, HubError> {
        self.require_clone()?;
        let (request, branch) = self.request_for(create)?;
        let keep_empty = create.keep_empty.unwrap_or(false);
        let title = format!(
            "analyse {} {} vs {} for {}",
            match request.kind {
                AnalysisKind::Release => "release",
                AnalysisKind::Patch => "patch",
            },
            request.source,
            request.compare,
            request.fix_version
        );
        let hub = Arc::clone(self);
        self.jobs.start(JobKind::Analysis, title, move |log| {
            let analysis = hub.analyse(&request, keep_empty, log)?;
            let n = hub.next_analysis.fetch_add(1, Ordering::Relaxed) + 1;
            let selection = analysis.default_selection();
            log.info(format!(
                "{} tickets, {} selected by default, {} uncoupled commits",
                analysis.tickets.len(),
                selection.len(),
                analysis.uncoupled.len()
            ));
            let mut store = lock(&hub.analyses);
            store.insert(
                n,
                Stored {
                    created: now_ms(),
                    branch,
                    analysis,
                    selection,
                    version: 0,
                },
            );
            hub.save_analyses(&store);
            Ok(JobResult::Analysis {
                analysis_id: analysis_id(n),
            })
        })
    }

    fn analyse(
        &self,
        request: &AnalysisRequest,
        keep_empty: bool,
        log: &JobLog,
    ) -> Result<Analysis, String> {
        let git = self.git();
        log.info("fetching origin");
        // An analysis of stale refs would be reviewed and released as if it were current: fail instead.
        git.fetch_all().map_err(|error| {
            format!("fetch from origin failed, so the analysis would use stale refs: {error}")
        })?;
        let tracker = match &self.tracker {
            Ok(tracker) => Some(tracker.as_ref()),
            Err(why) => {
                log.warn(format!("Jira is not available: {why}"));
                None
            }
        };
        let progress = |p: Progress| match p {
            Progress::Step(message) => log.info(message),
            Progress::Jira(done, total, key) => log.info(format!("Jira {done}/{total}: {key}")),
        };
        let analysis = Analyser {
            vcs: &git,
            rules: &self.rules,
            tracker,
            jira_base_url: self.config.jira_base_url.clone(),
            ticket_prefix: self.config.release.ticket_prefix.clone(),
            excluded: self.config.excluded_fix_versions.clone(),
            keep_empty,
            progress: &progress,
        }
        .analyse(request)
        .map_err(|e| e.to_string())?;
        let errors = analysis.jira_errors();
        if tracker.is_some() && !errors.is_empty() {
            log.warn(format!(
                "{} tickets without Jira data (first: {}: {})",
                errors.len(),
                errors[0].0,
                errors[0].1
            ));
        }
        Ok(analysis)
    }

    fn stored(&self, id: &str) -> Result<Stored, HubError> {
        id.strip_prefix('a')
            .and_then(|n| n.parse::<u64>().ok())
            .and_then(|n| lock(&self.analyses).get(&n).cloned())
            .ok_or_else(|| HubError::NotFound(format!("analysis {id}")))
    }

    fn is_stale(&self, analysis: &Analysis) -> bool {
        let git = self.git();
        let moved = |rev: &str, head: &str| git.rev_parse(rev).map_or(true, |h| h != head);
        moved(&analysis.request.source, &analysis.source_head)
            || moved(&analysis.request.compare, &analysis.compare_head)
    }

    fn view(&self, id: &str, stored: Stored) -> AnalysisView {
        AnalysisView {
            id: id.to_owned(),
            created: stored.created,
            stale: self.is_stale(&stored.analysis),
            branch: stored.branch,
            selection: stored.selection.into_iter().collect(),
            analysis: stored.analysis,
            version: stored.version,
        }
    }

    pub fn analysis(&self, id: &str) -> Result<AnalysisView, HubError> {
        let stored = self.stored(id)?;
        Ok(self.view(id, stored))
    }

    pub fn analyses(&self) -> Vec<AnalysisSummary> {
        lock(&self.analyses)
            .iter()
            .rev()
            .map(|(n, s)| AnalysisSummary {
                id: analysis_id(*n),
                created: s.created,
                kind: s.analysis.request.kind,
                fix_version: s.analysis.request.fix_version.clone(),
                branch: s.branch.clone(),
                tickets: count(s.analysis.tickets.len()),
                selected: count(s.selection.len()),
            })
            .collect()
    }

    pub fn set_selection(
        &self,
        id: &str,
        update: &SelectionUpdate,
    ) -> Result<AnalysisView, HubError> {
        let selection: BTreeSet<String> = update.selection.iter().cloned().collect();
        let stored = {
            let mut analyses = lock(&self.analyses);
            let stored = id
                .strip_prefix('a')
                .and_then(|n| n.parse::<u64>().ok())
                .and_then(|n| analyses.get_mut(&n))
                .ok_or_else(|| HubError::NotFound(format!("analysis {id}")))?;
            if let Some(version) = update.version
                && version != stored.version
            {
                return Err(HubError::Conflict(
                    "the board changed since you loaded it (someone else edited it): reload it"
                        .into(),
                ));
            }
            stored.analysis.validate_selection(&selection)?;
            stored.selection = selection;
            stored.version += 1;
            let copy = stored.clone();
            self.save_analyses(&analyses);
            copy
        };
        Ok(self.view(id, stored))
    }

    // ---- flows ----

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
            &self.rules,
            &branch,
            ScopeAction::Release { recreate: true },
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
            let outcome = outcome.map_err(|e| e.to_string())?;
            Ok(JobResult::Release { outcome })
        })
    }

    pub fn create_patch(
        self: &Arc<Self>,
        create: &PatchCreate,
        actor: Actor,
    ) -> Result<Job, HubError> {
        let stored = self.executable(&create.analysis_id, AnalysisKind::Patch, &actor)?;
        check_scope(&self.git(), &self.rules, &stored.branch, ScopeAction::Patch)?;
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
            .run(&stored.analysis, &stored.selection, dry_run, false);
            hub.tidy(&git, log);
            let outcome = outcome.map_err(|e| e.to_string())?;
            Ok(JobResult::Patch { outcome })
        })
    }

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
        check_scope(&self.git(), &self.rules, &branch, ScopeAction::Promote)?;
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
            let promoted = promoted.map_err(|e| e.to_string())?;
            Ok(JobResult::Promotion { promoted })
        })
    }

    // ---- announcements ----

    pub fn announcement(&self, id: &str, branch: Option<&str>) -> Result<Announcement, HubError> {
        let stored = self.stored(id)?;
        let branch = branch
            .filter(|b| !b.trim().is_empty())
            .unwrap_or(&stored.branch);
        Ok(Announcement::for_analysis(
            &stored.analysis,
            &stored.selection,
            branch,
        ))
    }

    pub fn send_announcement(&self, send: &AnnouncementSend) -> Result<AnnouncementSent, HubError> {
        let announcement = self.announcement(&send.analysis_id, send.branch.as_deref())?;
        let delivery =
            self.mailer
                .send(&announcement, &send.recipients)
                .map_err(|e| match e {
                    vdp_notify::NotifyError::NoRecipients
                    | vdp_notify::NotifyError::Address(..) => HubError::BadRequest(e.to_string()),
                    other => HubError::Internal(other.to_string()),
                })?;
        Ok(AnnouncementSent {
            delivery,
            announcement,
        })
    }
}

/// The clone in `dir`: cloned, origin, HEAD, last fetch and (while no job runs) its health.
pub(crate) fn repo_status_of(dir: &std::path::Path, busy: bool, protected: &str) -> RepoStatus {
    let git = GitCli::new(dir);
    let shown = dir.display().to_string();
    if !git.is_repository() {
        return RepoStatus {
            dir: shown,
            ..RepoStatus::default()
        };
    }
    let last_fetch = std::fs::metadata(dir.join(".git/FETCH_HEAD"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok());
    RepoStatus {
        dir: shown,
        cloned: true,
        origin_url: git.origin_url().ok().flatten(),
        head: git.rev_parse("HEAD").ok(),
        dirty: git.is_dirty().unwrap_or(false),
        last_fetch,
        health: if busy {
            Vec::new()
        } else {
            health(&git, protected)
        },
    }
}

/// What blocks or endangers the next job (only meaningful while no job runs). `protected`: a branch that must
/// never exist locally (dbt's source branch).
pub(crate) fn health(git: &GitCli, protected: &str) -> Vec<HealthProblem> {
    let state = git.workspace_state();
    let mut problems = Vec::new();
    let mut add = |code: &str, detail: String| {
        problems.push(HealthProblem {
            code: code.to_owned(),
            detail,
        });
    };
    if !state.head_ok {
        add(
            "broken",
            "HEAD is not a commit (an interrupted clone or a damaged repository): re-clone".into(),
        );
    }
    if let Some(operation) = &state.operation {
        add(
            "in_progress",
            format!("a {operation} was left in progress (an interrupted job)"),
        );
    }
    if state.index_lock {
        add(
            "index_lock",
            ".git/index.lock is left behind (a crashed git command)".into(),
        );
    }
    if !state.dirty.is_empty() {
        add(
            "dirty",
            format!("uncommitted changes in {}", state.dirty.join(", ")),
        );
    }
    if !state.local_branches.is_empty() {
        let source = protected;
        add(
            "local_branches",
            if state.local_branches.iter().any(|b| b == source) {
                format!(
                    "a local {source} exists: the hub keeps none (nothing may ever be committed on it)"
                )
            } else {
                format!(
                    "local branches left over: {}",
                    state.local_branches.join(", ")
                )
            },
        );
    }
    problems
}
