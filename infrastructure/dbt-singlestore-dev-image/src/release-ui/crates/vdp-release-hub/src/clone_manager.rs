//! Shared lifecycle management for a release-hub git clone (`Hub` and `VsHub`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vdp_git::{GitCli, Identity, Vcs, WriteGuard};

use crate::api::{CloneRequest, GitIdentity, HealthProblem, RepoStatus};
use crate::error::HubError;
use crate::hub::Actor;
use crate::jobs::{Job, JobKind, JobLog, JobResult, Jobs};

/// Configuration parameterizing how a hub owns and resets its local git clone.
#[derive(Debug, Clone)]
pub struct CloneConfig {
    pub repo_dir: PathBuf,
    pub repo_url: String,
    pub guard: WriteGuard,
    pub git_timeout: Option<std::time::Duration>,
    /// Remote-tracking ref that `detach` and `repair` reset HEAD to (e.g. `origin/dev` or `origin/main`).
    pub detach_target: String,
    /// Local branch flagged with the protected-branch warning in health checks (empty when none is special).
    pub protected_branch: String,
    /// When true, `git()` reads and injects the clone's local `user.name`/`user.email`.
    pub inject_clone_identity: bool,
    /// Error message when `require_clone` sees no repository yet.
    pub not_cloned_error: String,
    /// Error message when `clone_repo` sees a repository already exists.
    pub already_cloned_error: String,
}

/// Manages one local bare-remote git clone and its `Clone`/`Sync`/`Repair` jobs.
#[derive(Debug, Clone)]
pub struct CloneManager {
    config: CloneConfig,
    jobs: Arc<Jobs>,
}

impl CloneManager {
    /// Creates the manager of the clone described by `config`; its git work runs as jobs on `jobs`.
    pub fn new(config: CloneConfig, jobs: Arc<Jobs>) -> Self {
        Self { config, jobs }
    }

    /// The clone's configuration.
    pub fn config(&self) -> &CloneConfig {
        &self.config
    }

    /// The configured identity of the clone (`user.name`/`user.email`).
    ///
    /// # Errors
    ///
    /// [`HubError::Internal`] when git cannot read its configuration.
    pub fn identity(&self) -> Result<Option<GitIdentity>, HubError> {
        let git = GitCli::new(&self.config.repo_dir);
        if !git.is_repository() {
            return Ok(None);
        }
        Ok(git.identity()?.map(|i| GitIdentity {
            name: i.name,
            email: i.email,
        }))
    }

    /// A git client for the clone, with the write guard and timeout of the configuration and, when configured, the clone's
    /// identity.
    pub fn git(&self) -> GitCli {
        let mut git = GitCli::new(&self.config.repo_dir)
            .with_guard(self.config.guard.clone())
            .with_timeout(self.config.git_timeout);
        if self.config.inject_clone_identity {
            let identity = self.identity().ok().flatten().map(|i| Identity {
                name: i.name,
                email: i.email,
            });
            git = git.with_identity(identity);
        }
        git
    }

    /// A client that commits as `actor` and pushes where they may.
    pub fn git_as(&self, actor: &Actor) -> GitCli {
        GitCli::new(&self.config.repo_dir)
            .with_identity(actor.identity.clone())
            .with_guard(self.config.guard.clone())
            .with_timeout(self.config.git_timeout)
            .with_push_target(actor.push.clone())
    }

    /// The git client for the clone.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet.
    pub fn require_clone(&self) -> Result<GitCli, HubError> {
        let git = self.git();
        if git.is_repository() {
            Ok(git)
        } else {
            Err(HubError::Conflict(self.config.not_cloned_error.clone()))
        }
    }

    /// The state of the clone: directory, origin, HEAD, uncommitted changes and health problems.
    pub fn repo_status(&self) -> RepoStatus {
        repo_status_of(
            &self.config.repo_dir,
            self.jobs.active().is_some(),
            &self.config.protected_branch,
        )
    }

    /// Starts a job that clones the repository (`request.url`, else the configured one) and detaches HEAD.
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when there is no URL; [`HubError::Conflict`] when a clone exists or the directory is not
    /// empty; [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started. Failures of the clone itself end the job as failed.
    pub fn clone_repo(&self, request: &CloneRequest) -> Result<Job, HubError> {
        let url = request
            .url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| self.config.repo_url.clone());
        if url.trim().is_empty() {
            return Err(HubError::BadRequest("no repository URL".into()));
        }
        if self.git().is_repository() {
            return Err(HubError::Conflict(self.config.already_cloned_error.clone()));
        }
        let dir = &self.config.repo_dir;
        if std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some()) {
            return Err(HubError::Conflict(format!(
                "{} is not empty and not a git clone",
                dir.display()
            )));
        }
        let manager = self.clone();
        self.jobs
            .start(JobKind::Clone, format!("clone {url}"), move |log| {
                log.info(format!(
                    "cloning {url} into {}",
                    manager.config.repo_dir.display()
                ));
                let git = manager.git();
                git.clone_from(&url)
                    .map_err(|e| vdp_core::display_chain(&e))?;
                log.info("cloned");
                manager.detach(&git, log)?;
                Ok(JobResult::Repo {
                    repo: manager.repo_status(),
                })
            })
    }

    /// Deletes the clone (local only) and clones again.
    ///
    /// # Errors
    ///
    /// [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started. Failures of the clone itself end the job as failed.
    pub fn reclone(&self, request: &CloneRequest) -> Result<Job, HubError> {
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
        let manager = self.clone();
        self.jobs
            .start(JobKind::Clone, format!("re-clone {url}"), move |log| {
                let dir = &manager.config.repo_dir;
                log.info(format!("removing the clone in {}", dir.display()));
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let removed = if path.is_dir() {
                            std::fs::remove_dir_all(&path)
                        } else {
                            std::fs::remove_file(&path)
                        };
                        removed.map_err(|e| {
                            format!(
                                "cannot remove {}: {}",
                                path.display(),
                                vdp_core::display_chain(&e)
                            )
                        })?;
                    }
                }
                let git = manager.git();
                git.clone_from(&url)
                    .map_err(|e| vdp_core::display_chain(&e))?;
                log.info("cloned");
                manager.detach(&git, log)?;
                Ok(JobResult::Repo {
                    repo: manager.repo_status(),
                })
            })
    }

    /// Starts a job that fetches every branch and tag of origin (pruned); `on_synced` runs when the fetch is done.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started. A failing fetch ends the job as failed.
    pub fn sync(&self, on_synced: impl FnOnce() + Send + 'static) -> Result<Job, HubError> {
        self.require_clone()?;
        let manager = self.clone();
        self.jobs
            .start(JobKind::Sync, "sync with origin", move |log| {
                log.info("fetching origin (all branches, pruned, tags)");
                manager
                    .git()
                    .fetch_all()
                    .map_err(|e| vdp_core::display_chain(&e))?;
                on_synced();
                log.info("up to date");
                Ok(JobResult::Repo {
                    repo: manager.repo_status(),
                })
            })
    }

    /// Aborts what is in progress, removes a stale lock, discards local changes and branches, detaches HEAD at
    /// `detach_target`. Never touches origin.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started. A failing repair ends the job as failed.
    pub fn repair(&self) -> Result<Job, HubError> {
        self.require_clone()?;
        let manager = self.clone();
        self.jobs
            .start(JobKind::Repair, "repair the clone", move |log| {
                let git =
                    GitCli::new(&manager.config.repo_dir).with_timeout(manager.config.git_timeout);
                for action in git
                    .repair(&manager.config.detach_target)
                    .map_err(|e| vdp_core::display_chain(&e))?
                {
                    log.info(action);
                }
                Ok(JobResult::Repo {
                    repo: manager.repo_status(),
                })
            })
    }

    /// HEAD detached at `detach_target`, no local branches: every flow starts its branch from origin.
    ///
    /// # Errors
    ///
    /// The display chain of the git error when HEAD cannot be detached or the local branches cannot be deleted.
    pub fn detach(&self, git: &GitCli, log: &JobLog) -> Result<(), String> {
        let target = &self.config.detach_target;
        git.detach_at(target)
            .map_err(|e| vdp_core::display_chain(&e))?;
        let deleted = git
            .delete_local_branches()
            .map_err(|e| vdp_core::display_chain(&e))?;
        log.info(format!(
            "HEAD detached at {target}; local branches removed: {}",
            if deleted.is_empty() {
                "none".to_owned()
            } else {
                deleted.join(", ")
            }
        ));
        Ok(())
    }

    /// After a job: back to a detached HEAD without local branches. Returns `false` when in-progress
    /// work or uncommitted changes keep the state for inspection/repair.
    pub fn tidy(&self, git: &GitCli, log: &JobLog) -> bool {
        let state = git.workspace_state();
        if state.operation.is_some() || state.index_lock || !state.dirty.is_empty() {
            log.warn("the clone is left as it is: check its health (repair resets it)");
            return false;
        }
        if let Err(error) = self.detach(git, log) {
            log.warn(format!("could not tidy the clone: {error}"));
        }
        true
    }
}

/// The clone in `dir`: cloned, origin, HEAD, last fetch and (while no job runs) its health.
pub fn repo_status_of(dir: &Path, busy: bool, protected: &str) -> RepoStatus {
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
        add(
            "local_branches",
            if !protected.is_empty() && state.local_branches.iter().any(|b| b == protected) {
                format!(
                    "a local {protected} exists: the hub keeps none (nothing may ever be committed on it)"
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

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn git_cmd(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn tidy_detaches_clean_branch_and_skips_when_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let work = dir.path().join("work");
        let clone = dir.path().join("clone");
        let out = Command::new("git")
            .args(["init", "--bare"])
            .arg(&origin)
            .output()
            .unwrap();
        assert!(out.status.success());
        git_cmd(dir.path(), &["clone", origin.to_str().unwrap(), "work"]);
        git_cmd(&work, &["checkout", "-b", "dev"]);
        std::fs::write(work.join("f.txt"), "one\n").unwrap();
        git_cmd(&work, &["add", "f.txt"]);
        git_cmd(&work, &["commit", "-m", "init"]);
        git_cmd(&work, &["push", "-u", "origin", "dev"]);
        git_cmd(
            dir.path(),
            &["clone", origin.to_str().unwrap(), clone.to_str().unwrap()],
        );

        let jobs = Arc::new(Jobs::new(4, None));
        let mgr = CloneManager::new(
            CloneConfig {
                repo_dir: clone.clone(),
                repo_url: origin.to_string_lossy().into_owned(),
                guard: WriteGuard::default(),
                git_timeout: None,
                detach_target: "origin/dev".into(),
                protected_branch: "dev".into(),
                inject_clone_identity: true,
                not_cloned_error: "not cloned".into(),
                already_cloned_error: "already cloned".into(),
            },
            Arc::clone(&jobs),
        );

        let job = jobs
            .start(JobKind::Sync, "tidy", move |log| {
                let git = mgr.git();
                assert!(mgr.tidy(&git, log));
                assert_eq!(mgr.repo_status().health, vec![]);

                std::fs::write(clone.join("f.txt"), "dirty\n").unwrap();
                assert!(!mgr.tidy(&git, log));
                Ok(JobResult::Repo {
                    repo: mgr.repo_status(),
                })
            })
            .unwrap();
        for _ in 0..200 {
            let current = jobs.get(&job.id, 0).unwrap();
            if current.status != crate::jobs::JobStatus::Running {
                assert_eq!(current.status, crate::jobs::JobStatus::Done);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("tidy test job timed out");
    }
}
