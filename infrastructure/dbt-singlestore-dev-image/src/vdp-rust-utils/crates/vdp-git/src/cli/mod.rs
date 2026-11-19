//! [`Vcs`] over the system `git` binary in a work tree.

mod conflicts;
mod parse;
mod vcs;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::{GitError, Identity, PushTarget, Vcs, WorkspaceState, WriteGuard};

/// [`Vcs`] over the `git` binary in a work tree.
#[derive(Debug, Clone)]
pub struct GitCli {
    pub(super) work_tree: PathBuf,
    pub(super) remote: String,
    pub(super) identity: Option<Identity>,
    pub(super) guard: WriteGuard,
    timeout: Option<Duration>,
    pub(super) push_target: PushTarget,
}

impl GitCli {
    /// Creates a client for the work tree at `work_tree`: remote `origin`, no identity, an empty write guard and no timeout.
    pub fn new(work_tree: &Path) -> Self {
        Self {
            work_tree: work_tree.to_owned(),
            remote: "origin".into(),
            identity: None,
            guard: WriteGuard::default(),
            timeout: None,
            push_target: PushTarget::Origin,
        }
    }

    /// Pushes go to this target (fetches stay on `origin`).
    #[must_use]
    pub fn with_push_target(mut self, target: PushTarget) -> Self {
        self.push_target = target;
        self
    }

    /// Stops any git command that runs longer (a hung network or an ssh prompt would otherwise block forever).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs `command`, killing it after the timeout. Output is read on threads so a full pipe cannot block it.
    pub(super) fn output_of(&self, mut command: Command, label: &str) -> Result<Output, GitError> {
        let Some(limit) = self.timeout else {
            return Ok(command.output()?);
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let drain = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut buffer);
                }
                buffer
            })
        };
        let stdout = drain(
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let stderr = drain(
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let started = Instant::now();
        let mut pause = Duration::from_millis(1);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::Timeout {
                    command: label.to_owned(),
                    seconds: limit.as_secs(),
                });
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_millis(50));
        };
        Ok(Output {
            status,
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    /// Restricts the branches this client writes (see [`WriteGuard`]).
    #[must_use]
    pub fn with_guard(mut self, guard: WriteGuard) -> Self {
        self.guard = guard;
        self
    }

    /// What could block or endanger the next job in this work tree.
    pub fn workspace_state(&self) -> WorkspaceState {
        let operation = [
            ("MERGE_HEAD", "merge"),
            ("REVERT_HEAD", "revert"),
            ("CHERRY_PICK_HEAD", "cherry-pick"),
            ("rebase-merge", "rebase"),
            ("rebase-apply", "rebase"),
            ("sequencer", "revert or cherry-pick sequence"),
        ]
        .into_iter()
        .find(|(marker, _)| self.in_progress(marker))
        .map(|(_, name)| name.to_owned());
        let index_lock = self.in_progress("index.lock");
        let head_ok = self
            .output(&["rev-parse", "--verify", "--quiet", "HEAD"])
            .is_ok_and(|o| o.status.success());
        let dirty = self
            .run(&["status", "--porcelain", "--untracked-files=no"])
            .map(|text| {
                text.lines()
                    .map(|l| l.get(3..).unwrap_or(l).to_owned())
                    .take(20)
                    .collect()
            })
            .unwrap_or_default();
        let local_branches = self
            .run(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        WorkspaceState {
            operation,
            index_lock,
            head_ok,
            dirty,
            local_branches,
        }
    }

    /// Brings the work tree back to a clean, detached `rev`: aborts any operation in progress, removes a
    /// stale `index.lock` (only call this while no other git process runs), discards local changes and every
    /// local branch. Origin is never touched. Returns what it did.
    ///
    /// # Errors
    ///
    /// [`GitError::Spawn`] when the lock file cannot be removed or git cannot be started; [`GitError::Command`] when
    /// cleaning the work tree or detaching HEAD fails; [`GitError::Timeout`] when git outlives the client's timeout.
    /// Failures to abort an operation or to clear a sequence are ignored.
    pub fn repair(&self, rev: &str) -> Result<Vec<String>, GitError> {
        let mut done = Vec::new();
        let lock = self.work_tree.join(".git/index.lock");
        if lock.exists() {
            std::fs::remove_file(&lock)?;
            done.push("removed a stale .git/index.lock".to_owned());
        }
        for (marker, op) in [
            ("MERGE_HEAD", "merge"),
            ("REVERT_HEAD", "revert"),
            ("CHERRY_PICK_HEAD", "cherry-pick"),
            ("rebase-merge", "rebase"),
            ("rebase-apply", "rebase"),
        ] {
            if self.in_progress(marker) {
                let _ = self.run(&[op, "--abort"]);
                done.push(format!("aborted the {op} in progress"));
            }
        }
        if self.in_progress("sequencer") {
            let _ = self.run(&["cherry-pick", "--quit"]);
            let _ = self.run(&["revert", "--quit"]);
            done.push("cleared the revert/cherry-pick sequence".to_owned());
        }
        self.run(&["reset", "--quiet", "--hard"])?;
        self.run(&["clean", "-fdq"])?;
        self.detach_at(rev)?;
        done.push(format!("work tree clean, HEAD detached at {rev}"));
        let deleted = self.delete_local_branches()?;
        if !deleted.is_empty() {
            done.push(format!("local branches removed: {}", deleted.join(", ")));
        }
        Ok(done)
    }

    /// Commits go on the checked-out branch: it must be one the guard allows (a detached HEAD never is).
    pub(super) fn require_writable_head(&self, what: &str) -> Result<(), GitError> {
        match self.current_branch()? {
            Some(branch) => self.guard.check(&branch),
            None => Err(GitError::Guard(format!(
                "HEAD is detached: refusing to {what} outside a release branch"
            ))),
        }
    }

    /// Commits (reverts, cherry-picks, merges) are authored by `identity`, without touching any git config.
    #[must_use]
    pub fn with_identity(mut self, identity: Option<Identity>) -> Self {
        self.identity = identity.filter(Identity::is_complete);
        self
    }

    /// The work tree this client runs git in.
    pub fn work_tree(&self) -> &Path {
        &self.work_tree
    }

    pub(super) fn base(&self) -> Command {
        let mut command = Command::new("git");
        // Commit hooks (pre-commit, ...) must not run on automated reverts and cherry-picks.
        command
            .args(["-c", "core.hooksPath=/dev/null"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C");
        if let Some(identity) = &self.identity {
            command
                .arg("-c")
                .arg(format!("user.name={}", identity.name))
                .arg("-c")
                .arg(format!("user.email={}", identity.email));
        }
        command
    }

    pub(super) fn command(&self) -> Command {
        let mut command = self.base();
        command.arg("-C").arg(&self.work_tree);
        command
    }

    pub(super) fn output(&self, args: &[&str]) -> Result<Output, GitError> {
        let mut command = self.command();
        command.args(args);
        self.output_of(command, &args.join(" "))
    }

    pub(super) fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let output = self.output(args)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(failure(args, &output))
        }
    }

    pub(super) fn in_progress(&self, marker: &str) -> bool {
        self.run(&["rev-parse", "--git-path", marker])
            .is_ok_and(|path| {
                let path = PathBuf::from(path.trim());
                let path = if path.is_absolute() {
                    path
                } else {
                    self.work_tree.join(path)
                };
                path.exists()
            })
    }
}

pub(super) fn failure(args: &[&str], output: &Output) -> GitError {
    GitError::Command {
        command: args.join(" "),
        detail: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_owned(),
    }
}
