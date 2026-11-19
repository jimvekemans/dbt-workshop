//! Merge, cherry-pick, and revert conflict detection and automatic resolution on [`GitCli`].

use std::process::Output;

use super::{GitCli, failure};
use crate::{
    Applied, ApplyOptions, Choice, Conflict, ConflictKind, ConflictPath, GitError, OnConflict,
    Operation, Strategy, Vcs,
};

impl GitCli {
    /// `git revert` or `git cherry-pick` of `hash` (with `-m 1` for a merge: the whole feature against its
    /// mainline parent), or a `merge --no-ff`. With [`Strategy::TheirsHunks`], conflicting hunks take the
    /// change; what git leaves (a file changed on one side and deleted on the other) is resolved for the change
    /// too ([`OnConflict::Theirs`]) or left in progress for a decision ([`OnConflict::Pause`]).
    pub(super) fn apply(
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
                        tracing::warn!(
                            "could not resolve the conflicts of {op} {hash}: {}",
                            vdp_core::display_chain(&error)
                        );
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
    pub(super) fn describe_conflict(
        &self,
        operation: Operation,
        commit: &str,
    ) -> Result<Conflict, GitError> {
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

    pub(super) fn has_stage(&self, path: &str, stage: u8) -> Result<bool, GitError> {
        Ok(self
            .output(&["cat-file", "-e", &format!(":{stage}:{path}")])?
            .status
            .success())
    }

    /// Commits the operation in progress (its own message; no editor).
    pub(super) fn commit_in_progress(&self, operation: Operation) -> Result<Output, GitError> {
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
}

pub(super) fn is_empty_result(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("nothing to commit") || text.contains("previous cherry-pick is now empty")
}
