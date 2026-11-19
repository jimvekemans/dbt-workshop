//! [`Vcs`] trait implementation for [`GitCli`].

use std::path::Path;

use super::conflicts::is_empty_result;
use super::parse::{
    FIELD, FORMAT, RECORD, parse_log, parse_name_status, parse_numstat, split_name_status,
};
use super::{GitCli, failure};
use crate::{
    Applied, ApplyOptions, Choice, Commit, ConflictVersions, DiffEntry, FileChange, GitError,
    Identity, MergeTree, Operation, PathCommit, PushMode, PushRejection, PushTarget, RemoteBranch,
    Vcs,
};

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
