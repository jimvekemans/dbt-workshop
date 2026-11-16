use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::{
    Applied, ApplyOptions, ChangedPath, Choice, Commit, Conflict, ConflictKind, ConflictPath,
    ConflictVersions, DiffEntry, FileChange, GitError, Identity, MergeTree, OnConflict, Operation,
    PathCommit, PushMode, PushRejection, PushTarget, RemoteBranch, Strategy, Vcs, WorkspaceState,
    WriteGuard,
};

const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';
const FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ct%x1f%B%x1e";

/// [`Vcs`] over the `git` binary in a work tree.
#[derive(Debug, Clone)]
pub struct GitCli {
    work_tree: PathBuf,
    remote: String,
    identity: Option<Identity>,
    guard: WriteGuard,
    timeout: Option<Duration>,
    push_target: PushTarget,
}

impl GitCli {
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
    fn output_of(&self, mut command: Command, label: &str) -> Result<Output, GitError> {
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
    fn require_writable_head(&self, what: &str) -> Result<(), GitError> {
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

    pub fn work_tree(&self) -> &Path {
        &self.work_tree
    }

    fn base(&self) -> Command {
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

    fn command(&self) -> Command {
        let mut command = self.base();
        command.arg("-C").arg(&self.work_tree);
        command
    }

    fn output(&self, args: &[&str]) -> Result<Output, GitError> {
        let mut command = self.command();
        command.args(args);
        self.output_of(command, &args.join(" "))
    }

    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let output = self.output(args)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(failure(args, &output))
        }
    }

    /// `git revert` or `git cherry-pick` of `hash` (with `-m 1` for a merge: the whole feature against its
    /// mainline parent), or a `merge --no-ff`. With [`Strategy::TheirsHunks`], conflicting hunks take the
    /// change; what git leaves (a file changed on one side and deleted on the other) is resolved for the change
    /// too ([`OnConflict::Theirs`]) or left in progress for a decision ([`OnConflict::Pause`]).
    fn apply(
        &self,
        operation: Operation,
        hash: &str,
        message: Option<&str>,
        options: ApplyOptions,
    ) -> Result<Applied, GitError> {
        let op = operation.command();
        self.require_writable_head(op)?;
        let mut args = vec![op];
        match operation {
            Operation::Merge => args.extend(["--no-ff", "--no-edit"]),
            Operation::Revert | Operation::CherryPick => args.push("--no-edit"),
        }
        if options.strategy == Strategy::TheirsHunks {
            args.extend(["-X", "theirs"]);
        }
        if operation != Operation::Merge && self.commit(hash)?.is_merge() {
            args.extend(["-m", "1"]);
        }
        if let Some(message) = message {
            args.extend(["-m", message]);
        }
        args.push(hash);
        let output = self.output(&args)?;
        if output.status.success() {
            return Ok(Applied::Committed);
        }
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let busy = || self.in_progress(operation.marker()) || self.in_progress("sequencer");
        if is_empty_result(&text) && operation != Operation::Merge {
            // Leave nothing in progress behind.
            if busy() {
                let _ = self.run(&[op, "--skip"]);
            }
            return Ok(Applied::Empty);
        }
        if busy() {
            let conflict = self.describe_conflict(operation, hash)?;
            if !conflict.paths.is_empty() {
                if options.on_conflict == OnConflict::Pause {
                    return Ok(Applied::Conflicted(conflict));
                }
                match self.resolve_theirs(operation, hash) {
                    Ok(applied) => return Ok(applied),
                    Err(error) => {
                        tracing::warn!("could not resolve the conflicts of {op} {hash}: {error}");
                    }
                }
            }
            if busy() {
                let _ = self.run(&[op, "--abort"]);
            }
        }
        Err(failure(&args, &output))
    }

    /// The unmerged paths of the operation in progress.
    fn describe_conflict(&self, operation: Operation, commit: &str) -> Result<Conflict, GitError> {
        let listing = self.run(&["ls-files", "--unmerged", "-z"])?;
        let mut stages: std::collections::BTreeMap<String, Vec<u8>> =
            std::collections::BTreeMap::new();
        for entry in listing.split('\0').filter(|e| !e.is_empty()) {
            let Some((meta, path)) = entry.split_once('\t') else {
                continue;
            };
            let stage = meta
                .split_whitespace()
                .nth(2)
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(0);
            stages.entry(path.to_owned()).or_default().push(stage);
        }
        let paths = stages
            .into_iter()
            .map(|(path, mut present)| {
                present.sort_unstable();
                let kind = match present.as_slice() {
                    [1, 2, 3] => ConflictKind::Content,
                    [1, 2] => ConflictKind::ChangeDeletes,
                    [1, 3] => ConflictKind::BranchDeleted,
                    [2, 3] => ConflictKind::BothAdded,
                    _ => ConflictKind::Other,
                };
                ConflictPath { path, kind }
            })
            .collect();
        Ok(Conflict {
            operation,
            commit: commit.to_owned(),
            paths,
        })
    }

    fn has_stage(&self, path: &str, stage: u8) -> Result<bool, GitError> {
        Ok(self
            .output(&["cat-file", "-e", &format!(":{stage}:{path}")])?
            .status
            .success())
    }

    /// Commits the operation in progress (its own message; no editor).
    fn commit_in_progress(&self, operation: Operation) -> Result<Output, GitError> {
        let mut continued = self.command();
        continued
            .args([operation.command(), "--continue"])
            .env("GIT_EDITOR", "true");
        self.output_of(continued, &format!("{} --continue", operation.command()))
    }

    /// Resolves every unmerged path for the change being applied ("theirs": its version, or its deletion)
    /// and continues.
    fn resolve_theirs(&self, operation: Operation, commit: &str) -> Result<Applied, GitError> {
        let conflict = self.describe_conflict(operation, commit)?;
        for path in &conflict.paths {
            self.resolve_path(&path.path, Choice::Change)?;
        }
        match self.continue_op(operation, commit)? {
            Applied::Committed => Ok(Applied::Resolved(
                conflict.paths.into_iter().map(|p| p.path).collect(),
            )),
            other => Ok(other),
        }
    }

    fn in_progress(&self, marker: &str) -> bool {
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

fn failure(args: &[&str], output: &Output) -> GitError {
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

fn is_empty_result(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("nothing to commit") || text.contains("previous cherry-pick is now empty")
}

fn parse_log(text: &str) -> Result<Vec<Commit>, GitError> {
    text.split(RECORD)
        .map(|record| record.trim_start_matches('\n'))
        .filter(|record| !record.is_empty())
        .map(|record| {
            let fields: Vec<&str> = record.splitn(5, FIELD).collect();
            let [hash, parents, author, time, message] = fields[..] else {
                return Err(GitError::Parse(record.chars().take(80).collect()));
            };
            Ok(Commit {
                hash: hash.to_owned(),
                parents: parents.split_whitespace().map(str::to_owned).collect(),
                author: author.to_owned(),
                timestamp: time
                    .trim()
                    .parse()
                    .map_err(|_| GitError::Parse(time.to_owned()))?,
                message: message.trim_end().to_owned(),
            })
        })
        .collect()
}

/// `git diff --name-status -z`: status NUL path [NUL new path for renames/copies].
fn parse_name_status(text: &str) -> Vec<DiffEntry> {
    let mut fields = text.split('\0').filter(|f| !f.is_empty());
    let mut entries = Vec::new();
    while let Some(status) = fields.next() {
        let kind = status.chars().next().unwrap_or('M');
        let Some(first) = fields.next() else { break };
        let (path, old_path) = if matches!(kind, 'R' | 'C') {
            match fields.next() {
                Some(new) => (new.to_owned(), Some(first.to_owned())),
                None => (first.to_owned(), None),
            }
        } else {
            (first.to_owned(), None)
        };
        entries.push(DiffEntry {
            status: kind,
            path,
            old_path,
            additions: None,
            deletions: None,
        });
    }
    entries
}

/// `git diff --numstat -z`: `added TAB deleted TAB path NUL`, or for a rename `added TAB deleted TAB NUL old NUL
/// new NUL`; `-` counts mean a binary file.
fn parse_numstat(text: &str) -> Vec<(String, Option<u64>, Option<u64>)> {
    let mut fields = text.split('\0');
    let mut out = Vec::new();
    while let Some(field) = fields.next() {
        if field.is_empty() {
            continue;
        }
        let mut parts = field.splitn(3, '\t');
        let added = parts.next().and_then(|n| n.parse().ok());
        let deleted = parts.next().and_then(|n| n.parse().ok());
        let path = match parts.next() {
            Some(path) if !path.is_empty() => path.to_owned(),
            // A rename: the old and the new path follow as their own fields.
            _ => {
                let _old = fields.next();
                fields.next().unwrap_or_default().to_owned()
            }
        };
        out.push((path, added, deleted));
    }
    out
}

/// Splits what follows a log record into its leading name-status lines and the next record's text.
fn split_name_status(chunk: &str) -> (Vec<ChangedPath>, &str) {
    let mut files = Vec::new();
    let mut offset = 0;
    for line in chunk.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n');
        if trimmed.is_empty() {
            offset += line.len();
            continue;
        }
        let mut fields = trimmed.split('\t');
        let status = fields.next().unwrap_or_default();
        let valid = !status.is_empty()
            && status.len() <= 4
            && status.chars().next().is_some_and(|c| "AMDRCTU".contains(c))
            && status.chars().skip(1).all(|c| c.is_ascii_digit());
        let paths: Vec<&str> = fields.collect();
        if !valid || paths.is_empty() {
            break;
        }
        files.push(ChangedPath {
            status: status.chars().next().unwrap_or('M'),
            path: (*paths.last().unwrap_or(&"")).to_owned(),
        });
        offset += line.len();
    }
    (files, &chunk[offset..])
}

impl Vcs for GitCli {
    fn fetch(&self) -> Result<(), GitError> {
        self.run(&["fetch", "--prune", &self.remote]).map(|_| ())
    }

    fn merge_base(&self, a: &str, b: &str) -> Result<String, GitError> {
        Ok(self.run(&["merge-base", a, b])?.trim().to_owned())
    }

    fn log(&self, from: &str, to: &str, first_parent: bool) -> Result<Vec<Commit>, GitError> {
        let range = format!("{from}..{to}");
        let mut args = vec!["log", "--reverse", FORMAT];
        if first_parent {
            args.push("--first-parent");
        }
        args.push(&range);
        parse_log(&self.run(&args)?)
    }

    fn recent(&self, rev: &str, limit: usize, first_parent: bool) -> Result<Vec<Commit>, GitError> {
        let count = format!("--max-count={limit}");
        let mut args = vec!["log", FORMAT, &count];
        if first_parent {
            args.push("--first-parent");
        }
        args.extend([rev, "--"]);
        parse_log(&self.run(&args)?)
    }

    fn commit(&self, rev: &str) -> Result<Commit, GitError> {
        parse_log(&self.run(&["log", "-1", FORMAT, rev])?)?
            .pop()
            .ok_or_else(|| GitError::Parse(format!("no commit {rev}")))
    }

    fn is_dirty(&self) -> Result<bool, GitError> {
        Ok(!self
            .run(&["status", "--porcelain", "--untracked-files=no"])?
            .trim()
            .is_empty())
    }

    fn branch_exists(&self, name: &str) -> Result<bool, GitError> {
        let reference = format!("refs/heads/{name}");
        Ok(self
            .output(&["show-ref", "--verify", "--quiet", &reference])?
            .status
            .success())
    }

    fn create_branch(&self, name: &str, start: &str, force: bool) -> Result<(), GitError> {
        self.guard.check(name)?;
        let flag = if force { "-B" } else { "-b" };
        self.run(&["checkout", "--no-track", flag, name, start])
            .map(|_| ())
    }

    fn checkout(&self, name: &str) -> Result<(), GitError> {
        self.guard.check(name)?;
        self.run(&["checkout", name]).map(|_| ())
    }

    fn revert_with(&self, hash: &str, options: ApplyOptions) -> Result<Applied, GitError> {
        self.apply(Operation::Revert, hash, None, options)
    }

    fn cherry_pick_with(&self, hash: &str, options: ApplyOptions) -> Result<Applied, GitError> {
        self.apply(Operation::CherryPick, hash, None, options)
    }

    fn merge_with(
        &self,
        rev: &str,
        message: &str,
        options: ApplyOptions,
    ) -> Result<Applied, GitError> {
        self.apply(Operation::Merge, rev, Some(message), options)
    }

    fn conflict_versions(&self, path: &str, limit: usize) -> Result<ConflictVersions, GitError> {
        let mut versions = ConflictVersions::default();
        let mut read = |stage: u8| -> Result<Option<String>, GitError> {
            let output = self.output(&["show", &format!(":{stage}:{path}")])?;
            if !output.status.success() {
                return Ok(None);
            }
            let mut bytes = output.stdout;
            if bytes.len() > limit {
                bytes.truncate(limit);
                versions.truncated = true;
            }
            match String::from_utf8(bytes) {
                Ok(text) if !text.contains('\0') => Ok(Some(text)),
                Ok(_) | Err(_) => {
                    versions.binary = true;
                    Ok(Some(String::new()))
                }
            }
        };
        let base = read(1)?;
        let branch = read(2)?;
        let change = read(3)?;
        versions.base = base;
        versions.branch = branch;
        versions.change = change;
        Ok(versions)
    }

    fn resolve_path(&self, path: &str, choice: Choice) -> Result<(), GitError> {
        self.require_writable_head("resolve")?;
        let (stage, side) = match choice {
            Choice::Change => (3, "--theirs"),
            Choice::Branch => (2, "--ours"),
            Choice::Delete => (0, ""),
        };
        if stage != 0 && self.has_stage(path, stage)? {
            self.run(&["checkout", side, "--", path])?;
            self.run(&["add", "--", path])?;
        } else {
            self.run(&["rm", "--quiet", "--force", "--ignore-unmatch", "--", path])?;
        }
        Ok(())
    }

    fn continue_op(&self, operation: Operation, commit: &str) -> Result<Applied, GitError> {
        self.require_writable_head("continue")?;
        let left = self.describe_conflict(operation, commit)?;
        if !left.paths.is_empty() {
            return Ok(Applied::Conflicted(left));
        }
        let output = self.commit_in_progress(operation)?;
        if output.status.success() {
            return Ok(Applied::Committed);
        }
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if is_empty_result(&text) && operation != Operation::Merge {
            let _ = self.run(&[operation.command(), "--skip"]);
            return Ok(Applied::Empty);
        }
        Err(failure(&[operation.command(), "--continue"], &output))
    }

    fn skip_op(&self, operation: Operation) -> Result<(), GitError> {
        if operation == Operation::Merge {
            return self.abort_op(operation);
        }
        self.run(&[operation.command(), "--skip"]).map(|_| ())
    }

    fn abort_op(&self, operation: Operation) -> Result<(), GitError> {
        if self.in_progress(operation.marker()) || self.in_progress("sequencer") {
            self.run(&[operation.command(), "--abort"])?;
        }
        Ok(())
    }

    fn push(&self, branch: &str, mode: PushMode) -> Result<(), GitError> {
        self.guard.check(branch)?;
        let lease = match &mode {
            PushMode::Normal => None,
            PushMode::ForceWithLease => Some("--force-with-lease".to_owned()),
            PushMode::Expect(expected) => Some(format!(
                "--force-with-lease=refs/heads/{branch}:{}",
                expected.as_deref().unwrap_or_default()
            )),
        };
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        let destination = match &self.push_target {
            PushTarget::Origin => self.remote.clone(),
            PushTarget::Url { url, .. } => url.clone(),
        };
        let mut args = vec!["push", "--porcelain", destination.as_str()];
        if let Some(lease) = &lease {
            args.push(lease);
        }
        args.push(&refspec);
        let mut command = self.command();
        command.args(&args);
        if let PushTarget::Url { token, .. } = &self.push_target {
            use base64::Engine as _;
            let basic = base64::engine::general_purpose::STANDARD.encode(format!("oauth2:{token}"));
            command
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "http.extraHeader")
                .env(
                    "GIT_CONFIG_VALUE_0",
                    format!("Authorization: Basic {basic}"),
                );
        }
        let output = self.output_of(command, &format!("push {branch}"))?;
        let follow = || {
            // A push to a URL does not move origin's tracking ref: fetch it.
            self.run(&[
                "fetch",
                "--quiet",
                self.remote.as_str(),
                &format!("+refs/heads/{branch}:refs/remotes/{}/{branch}", self.remote),
            ])
        };
        if output.status.success() {
            if matches!(self.push_target, PushTarget::Url { .. }) {
                let _ = follow();
            }
            return Ok(());
        }
        let detail = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_owned();
        // The answer may have been lost after the remote accepted it: look before reporting a failure.
        let fetched = follow();
        if fetched.is_ok()
            && let (Ok(local), Ok(Some(remote))) = (
                self.rev_parse(&format!("refs/heads/{branch}")),
                self.remote_head(branch),
            )
            && local == remote
        {
            tracing::info!("push of {branch} reported a failure, but origin has it: {detail}");
            return Ok(());
        }
        Err(GitError::Push {
            branch: branch.to_owned(),
            rejection: PushRejection::classify(&detail),
            detail,
        })
    }

    fn clone_from(&self, url: &str) -> Result<(), GitError> {
        std::fs::create_dir_all(&self.work_tree)?;
        let target = self.work_tree.to_string_lossy().into_owned();
        let args = ["clone", "--no-single-branch", url, target.as_str()];
        let mut command = self.base();
        command.args(args);
        let output = self.output_of(command, "clone")?;
        if output.status.success() {
            Ok(())
        } else {
            Err(failure(&args, &output))
        }
    }

    fn is_repository(&self) -> bool {
        self.work_tree.join(".git").exists()
    }

    fn fetch_all(&self) -> Result<(), GitError> {
        self.run(&["fetch", "--all", "--prune", "--force", "--tags"])
            .map(|_| ())
    }

    fn rev_parse(&self, rev: &str) -> Result<String, GitError> {
        let spec = format!("{rev}^{{commit}}");
        Ok(self
            .run(&["rev-parse", "--verify", "--quiet", &spec])?
            .trim()
            .to_owned())
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, GitError> {
        let output = self.output(&["merge-base", "--is-ancestor", ancestor, descendant])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(failure(
                &["merge-base", "--is-ancestor", ancestor, descendant],
                &output,
            )),
        }
    }

    fn remote_branches(&self, prefix: &str) -> Result<Vec<RemoteBranch>, GitError> {
        let pattern = format!("refs/remotes/{}/{prefix}", self.remote);
        let text = self.run(&[
            "for-each-ref",
            "--format=%(refname)%1f%(objectname)%1f%(committerdate:unix)%1f%(contents:subject)",
            &pattern,
        ])?;
        let strip = format!("refs/remotes/{}/", self.remote);
        Ok(text
            .lines()
            .filter_map(|line| {
                let mut fields = line.splitn(4, FIELD);
                let name = fields.next()?.strip_prefix(&strip)?.to_owned();
                let head = fields.next()?.to_owned();
                let timestamp = fields.next()?.trim().parse().unwrap_or(0);
                let subject = fields.next().unwrap_or_default().to_owned();
                Some(RemoteBranch {
                    name,
                    head,
                    timestamp,
                    subject,
                })
            })
            .collect())
    }

    fn checkout_reset(&self, name: &str, start: &str) -> Result<(), GitError> {
        self.guard.check(name)?;
        self.run(&["checkout", "--no-track", "-B", name, start])
            .map(|_| ())
    }

    fn reset_hard(&self, rev: &str) -> Result<(), GitError> {
        self.require_writable_head("reset")?;
        self.run(&["reset", "--hard", rev]).map(|_| ())
    }

    fn merge_no_ff(&self, rev: &str, message: &str) -> Result<(), GitError> {
        self.require_writable_head("merge")?;
        let args = ["merge", "--no-ff", "--no-edit", "-m", message, rev];
        let output = self.output(&args)?;
        if output.status.success() {
            return Ok(());
        }
        if self.in_progress("MERGE_HEAD") {
            let _ = self.run(&["merge", "--abort"]);
        }
        Err(failure(&args, &output))
    }

    fn identity(&self) -> Result<Option<Identity>, GitError> {
        if let Some(identity) = &self.identity {
            return Ok(Some(identity.clone()));
        }
        let get = |key: &str| -> Result<String, GitError> {
            let output = self.output(&["config", "--local", "--get", key])?;
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        };
        let identity = Identity {
            name: get("user.name")?,
            email: get("user.email")?,
        };
        Ok(identity.is_complete().then_some(identity))
    }

    fn set_identity(&self, identity: &Identity) -> Result<(), GitError> {
        self.run(&["config", "user.name", &identity.name])?;
        self.run(&["config", "user.email", &identity.email])
            .map(|_| ())
    }

    fn origin_url(&self) -> Result<Option<String>, GitError> {
        let output = self.output(&["remote", "get-url", self.remote.as_str()])?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    }

    fn cherry(&self, upstream: &str, head: &str) -> Result<Vec<(String, bool)>, GitError> {
        Ok(self
            .run(&["cherry", upstream, head])?
            .lines()
            .filter_map(|line| {
                let (mark, hash) = line.trim().split_once(' ')?;
                Some((hash.trim().to_owned(), mark == "-"))
            })
            .collect())
    }

    fn touching(&self, from: &str, to: &str, path: &str) -> Result<Vec<String>, GitError> {
        let range = format!("{from}..{to}");
        Ok(self
            .run(&[
                "log",
                "--first-parent",
                "-m",
                "--format=%H",
                &range,
                "--",
                path,
            ])?
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }

    fn commit_files(
        &self,
        files: &[(String, String)],
        message: &str,
    ) -> Result<Option<String>, GitError> {
        self.require_writable_head("commit")?;
        let mut paths: Vec<&str> = Vec::new();
        for (path, content) in files {
            let relative = Path::new(path);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Err(GitError::Guard(format!(
                    "{path} is not a path inside the repository"
                )));
            }
            let full = self.work_tree.join(relative);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full, content)?;
            paths.push(path);
        }
        let mut add = vec!["add", "--"];
        add.extend(paths.iter().copied());
        self.run(&add)?;
        let staged = self.output(&["diff", "--cached", "--quiet"])?;
        if staged.status.success() {
            return Ok(None);
        }
        self.run(&["commit", "--quiet", "-m", message])?;
        Ok(Some(self.rev_parse("HEAD")?))
    }

    fn show_file(&self, rev: &str, path: &str) -> Result<Option<String>, GitError> {
        let output = self.output(&["show", &format!("{rev}:{path}")])?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
    }

    fn diff_files(&self, from: &str, to: &str) -> Result<Vec<DiffEntry>, GitError> {
        let status = self.run(&["diff", "-M", "--name-status", "-z", from, to, "--"])?;
        let numstat = self.run(&["diff", "-M", "--numstat", "-z", from, to, "--"])?;
        let mut entries = parse_name_status(&status);
        for (path, additions, deletions) in parse_numstat(&numstat) {
            if let Some(entry) = entries.iter_mut().find(|e| e.path == path) {
                entry.additions = additions;
                entry.deletions = deletions;
            }
        }
        Ok(entries)
    }

    fn diff_text(&self, from: &str, to: &str, path: &str) -> Result<String, GitError> {
        self.run(&[
            "diff",
            "-M",
            "--no-color",
            "--no-ext-diff",
            from,
            to,
            "--",
            path,
        ])
    }

    fn commits_touching(&self, from: &str, to: &str) -> Result<Vec<PathCommit>, GitError> {
        let range = format!("{from}..{to}");
        let text = self.run(&[
            "log",
            "--reverse",
            "--no-merges",
            "--name-status",
            FORMAT,
            &range,
        ])?;
        let mut out: Vec<PathCommit> = Vec::new();
        for chunk in text.split(RECORD) {
            let (files, rest) = split_name_status(chunk);
            if let Some(last) = out.last_mut() {
                last.files.extend(files);
            }
            if !rest.trim().is_empty()
                && let Some(commit) = parse_log(&format!("{rest}{RECORD}"))?.pop()
            {
                out.push(PathCommit {
                    commit,
                    files: Vec::new(),
                });
            }
        }
        Ok(out)
    }

    fn merge_tree(&self, ours: &str, theirs: &str) -> Result<MergeTree, GitError> {
        let args = [
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            ours,
            theirs,
        ];
        let output = self.output(&args)?;
        // Exit 0: clean, 1: conflicts (the tree is still written); anything else is an error.
        if !matches!(output.status.code(), Some(0 | 1)) {
            return Err(failure(&args, &output));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut lines = text.lines();
        let tree = lines
            .next()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| GitError::Parse("merge-tree wrote no tree".into()))?
            .to_owned();
        let mut conflicts: Vec<String> = lines
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        conflicts.dedup();
        Ok(MergeTree { tree, conflicts })
    }

    fn blob_id(&self, rev: &str, path: &str) -> Result<Option<String>, GitError> {
        let output =
            self.output(&["rev-parse", "--verify", "--quiet", &format!("{rev}:{path}")])?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    }

    fn show_bytes(&self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let output = self.output(&["cat-file", "blob", &format!("{rev}:{path}")])?;
        Ok(output.status.success().then_some(output.stdout))
    }

    fn commit_changes(
        &self,
        changes: &[FileChange],
        message: &str,
    ) -> Result<Option<String>, GitError> {
        self.require_writable_head("commit")?;
        let mut paths: Vec<&str> = Vec::new();
        for change in changes {
            let relative = Path::new(&change.path);
            if change.path.is_empty()
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                || relative.starts_with(".git")
            {
                return Err(GitError::Guard(format!(
                    "{} is not a path inside the repository",
                    change.path
                )));
            }
            let full = self.work_tree.join(relative);
            match &change.content {
                Some(content) => {
                    if let Some(parent) = full.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&full, content)?;
                }
                None if full.exists() => std::fs::remove_file(&full)?,
                None => {}
            }
            paths.push(&change.path);
        }
        let mut add = vec!["add", "--all", "--"];
        add.extend(paths.iter().copied());
        self.run(&add)?;
        let staged = self.output(&["diff", "--cached", "--quiet"])?;
        if staged.status.success() {
            return Ok(None);
        }
        self.run(&["commit", "--quiet", "-m", message])?;
        Ok(Some(self.rev_parse("HEAD")?))
    }

    fn path_log(&self, from: &str, to: &str, dir: &str) -> Result<Vec<PathCommit>, GitError> {
        let range = format!("{from}..{to}");
        let text = self.run(&[
            "log",
            "--reverse",
            "--first-parent",
            "-m",
            "--name-status",
            FORMAT,
            &range,
            "--",
            dir,
        ])?;
        // Each record is the commit, then (after the record separator) its name-status lines.
        let mut out: Vec<PathCommit> = Vec::new();
        for chunk in text.split(RECORD) {
            let (files, rest) = split_name_status(chunk);
            if let Some(last) = out.last_mut() {
                last.files.extend(files);
            }
            if !rest.trim().is_empty()
                && let Some(commit) = parse_log(&format!("{rest}{RECORD}"))?.pop()
            {
                out.push(PathCommit {
                    commit,
                    files: Vec::new(),
                });
            }
        }
        out.retain(|c| !c.files.is_empty());
        Ok(out)
    }

    fn files_at(&self, rev: &str, dir: &str) -> Result<Vec<String>, GitError> {
        Ok(self
            .run(&["ls-tree", "-r", "--name-only", rev, "--", dir])?
            .lines()
            .map(str::to_owned)
            .collect())
    }

    fn rebase_onto(&self, onto: &str, upstream: &str) -> Result<(), GitError> {
        self.require_writable_head("rebase")?;
        let args = ["rebase", "--onto", onto, upstream];
        let output = self.output(&args)?;
        if output.status.success() {
            return Ok(());
        }
        let _ = self.run(&["rebase", "--abort"]);
        Err(failure(&args, &output))
    }

    fn remote_head(&self, branch: &str) -> Result<Option<String>, GitError> {
        let reference = format!("refs/remotes/{}/{branch}", self.remote);
        let output = self.output(&["rev-parse", "--verify", "--quiet", &reference])?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    }

    fn current_branch(&self) -> Result<Option<String>, GitError> {
        let output = self.output(&["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    }

    fn detach_at(&self, rev: &str) -> Result<(), GitError> {
        self.run(&["checkout", "--quiet", "--force", "--detach", rev])
            .map(|_| ())
    }

    fn delete_local_branches(&self) -> Result<Vec<String>, GitError> {
        if self.current_branch()?.is_some() {
            return Err(GitError::Guard(
                "detach HEAD before deleting the local branches".into(),
            ));
        }
        let names: Vec<String> = self
            .run(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])?
            .lines()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .collect();
        for name in &names {
            self.run(&["branch", "--quiet", "-D", name])?;
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_records() {
        let text = format!(
            "a1{FIELD}p1 p2{FIELD}Jane{FIELD}1700000000{FIELD}Merge branch 'feature/INTEL-1'\n\nbody\n{RECORD}\nb2{FIELD}a1{FIELD}Joe{FIELD}1700000100{FIELD}fix{RECORD}"
        );
        let commits = parse_log(&text).unwrap();
        assert_eq!(commits.len(), 2);
        assert!(commits[0].is_merge());
        assert_eq!(commits[0].message, "Merge branch 'feature/INTEL-1'\n\nbody");
        assert_eq!(commits[1].author, "Joe");
    }
}
