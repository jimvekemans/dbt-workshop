//! Git for the release flow, through the system `git` client.
//!
//! The Python utilities used GitPython. The system client is what developers and CI run anyway, it handles
//! every conflict and hook case the same way as on the command line, and it needs no native library.

mod cli;
mod error;

use std::fmt::Debug;

pub use cli::GitCli;
pub use error::GitError;

/// One commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    /// Committer time, seconds since the epoch.
    pub timestamp: i64,
    pub message: String,
}

/// A file to write (`Some`) or delete (`None`) in a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub content: Option<Vec<u8>>,
}

/// How a commit changed a file: `A`dded, `M`odified, `D`eleted, `R`enamed (to `path`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedPath {
    pub status: char,
    pub path: String,
}

/// One file in a diff ([`Vcs::diff_files`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    /// `A`dded, `M`odified, `D`eleted, `R`enamed, `C`opied, `T`ype changed.
    pub status: char,
    pub path: String,
    /// The path before a rename or copy.
    pub old_path: Option<String>,
    /// Lines added and removed; `None` for a binary file.
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

/// The result of [`Vcs::merge_tree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeTree {
    /// The resulting tree (with conflict markers in conflicting files).
    pub tree: String,
    /// Paths that would conflict.
    pub conflicts: Vec<String>,
}

/// A commit with the files it changed (see [`Vcs::path_log`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCommit {
    pub commit: Commit,
    pub files: Vec<ChangedPath>,
}

impl Commit {
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// The first line of the message.
    pub fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or_default()
    }

    pub fn short(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }
}

/// How a revert or cherry-pick went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// A new commit was made.
    Committed,
    /// The change was already there; nothing was committed (and no operation is left in progress).
    Empty,
    /// Committed after resolving conflicts in these paths for the change being applied (its version of a
    /// file, or its deletion), as `-X theirs` does for conflicting hunks.
    Resolved(Vec<String>),
    /// Stopped on a conflict ([`OnConflict::Pause`]): the operation is left in progress for a decision
    /// ([`Vcs::resolve_path`], then [`Vcs::continue_op`], [`Vcs::skip_op`] or [`Vcs::abort_op`]).
    Conflicted(Conflict),
}

/// The git operation a conflict belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Revert,
    CherryPick,
    Merge,
}

impl Operation {
    pub const fn command(self) -> &'static str {
        match self {
            Self::Revert => "revert",
            Self::CherryPick => "cherry-pick",
            Self::Merge => "merge",
        }
    }

    pub(crate) const fn marker(self) -> &'static str {
        match self {
            Self::Revert => "REVERT_HEAD",
            Self::CherryPick => "CHERRY_PICK_HEAD",
            Self::Merge => "MERGE_HEAD",
        }
    }
}

/// What to do with conflicts git leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConflict {
    /// Resolve every leftover conflict for the change being applied.
    #[default]
    Theirs,
    /// Stop and leave the operation in progress ([`Applied::Conflicted`]).
    Pause,
}

/// How git merges conflicting hunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// `-X theirs`: conflicting hunks take the change being applied (Python's behaviour); only whole-file
    /// conflicts (a file changed on one side and deleted on the other) remain.
    #[default]
    TheirsHunks,
    /// No `-X theirs`: every conflicting hunk is a conflict.
    Strict,
}

/// How a revert, cherry-pick or merge handles conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApplyOptions {
    pub on_conflict: OnConflict,
    pub strategy: Strategy,
}

/// The shape of a conflicting path. "Branch" is the checked-out branch, "change" the commit being applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both changed the same lines.
    Content,
    /// The change deletes the file, which the branch changed.
    ChangeDeletes,
    /// The branch deleted the file, which the change modifies.
    BranchDeleted,
    /// Both added the file.
    BothAdded,
    Other,
}

/// One conflicting path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictPath {
    pub path: String,
    pub kind: ConflictKind,
}

/// An operation stopped on conflicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub operation: Operation,
    /// The commit being applied (or merged).
    pub commit: String,
    pub paths: Vec<ConflictPath>,
}

/// How to resolve one conflicting path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The change's version (or its deletion).
    Change,
    /// The branch's version (or its deletion): this path stays as it is.
    Branch,
    /// Delete the file.
    Delete,
}

/// The three versions of a conflicting path (each `None` when absent on that side).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConflictVersions {
    pub base: Option<String>,
    pub branch: Option<String>,
    pub change: Option<String>,
    /// Not valid UTF-8: shown as binary.
    pub binary: bool,
    /// A version was cut at the size limit.
    pub truncated: bool,
}

/// A remote-tracking branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranch {
    /// Without the remote, e.g. `release/2026_DROP24`.
    pub name: String,
    pub head: String,
    pub timestamp: i64,
    pub subject: String,
}

/// Who commits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

impl Identity {
    pub fn is_complete(&self) -> bool {
        !self.name.trim().is_empty() && self.email.contains('@')
    }
}

/// The state of a work tree, for health checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceState {
    /// A merge, revert, cherry-pick or rebase left in progress.
    pub operation: Option<String>,
    /// `.git/index.lock` exists (a git process crashed, or one runs right now).
    pub index_lock: bool,
    /// HEAD resolves to a commit (false: an interrupted clone or a damaged repository).
    pub head_ok: bool,
    /// Tracked files with uncommitted changes (at most 20).
    pub dirty: Vec<String>,
    pub local_branches: Vec<String>,
}

/// Which branches a client may write: commit on (revert, cherry-pick, merge), reset, create, check out for
/// writing, or push.
///
/// The release flows must never change the source branch (`dev`, the tested dev environment): not even
/// locally, so that a later push can never carry a revert there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteGuard {
    /// Never written, whatever else says (the source branch).
    pub protected: Vec<String>,
    /// When not empty, the only branches that may be written: exact names, or prefixes ending in `/`.
    pub writable: Vec<String>,
}

impl WriteGuard {
    /// `Err` with the reason when `branch` may not be written.
    pub fn check(&self, branch: &str) -> Result<(), GitError> {
        let branch = branch.trim();
        let branch = branch
            .strip_prefix("refs/heads/")
            .or_else(|| branch.strip_prefix("origin/"))
            .unwrap_or(branch);
        if self.protected.iter().any(|p| p == branch) {
            return Err(GitError::Guard(format!(
                "{branch} is protected: the release tools never commit on it or push it"
            )));
        }
        let allowed = self.writable.is_empty()
            || self.writable.iter().any(|w| {
                if w.ends_with('/') {
                    branch.starts_with(w.as_str()) && branch.len() > w.len()
                } else {
                    w == branch
                }
            });
        if allowed {
            Ok(())
        } else {
            Err(GitError::Guard(format!(
                "{branch} is not a branch the release tools may write (allowed: {})",
                self.writable.join(", ")
            )))
        }
    }
}

/// Where pushes go.
#[derive(Clone, Default, PartialEq, Eq)]
pub enum PushTarget {
    /// The clone's `origin`, with whatever credentials it has (the CLI: the developer's own).
    #[default]
    Origin,
    /// An HTTPS URL as a person: their token goes to git in the child's environment (`http.extraHeader`),
    /// never in its arguments or on disk. The remote enforces that person's rights.
    Url { url: String, token: String },
}

impl std::fmt::Debug for PushTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Origin => f.write_str("Origin"),
            Self::Url { url, .. } => write!(f, "Url({url}, token: <redacted>)"),
        }
    }
}

/// How a push may replace the remote branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushMode {
    /// Fast-forward only.
    Normal,
    /// Replace the branch, but only if it is still where our last fetch saw it.
    ForceWithLease,
    /// Replace the branch only if origin has it at exactly this commit (`None`: only if origin does not have
    /// it): the lease is bound to what a preview showed, not to whatever the last fetch saw.
    Expect(Option<String>),
}

/// Why the remote refused a push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushRejection {
    /// The branch has commits the push does not contain.
    NonFastForward,
    /// The branch moved since the expected commit (the lease).
    StaleLease,
    /// A protected branch: no (force) push for this account.
    Protected,
    /// A server-side hook declined it (e.g. a secret scanner).
    HookDeclined,
    /// The credentials were refused.
    Auth,
    /// The remote could not be reached.
    Network,
    Other,
}

impl PushRejection {
    /// Reads git's push output.
    pub fn classify(output: &str) -> Self {
        let text = output.to_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
        if has(&["stale info"]) {
            Self::StaleLease
        } else if has(&[
            "protected branch",
            "not allowed to force push",
            "not allowed to push",
            "you are not allowed",
        ]) {
            Self::Protected
        } else if has(&[
            "pre-receive hook declined",
            "hook declined",
            "remote rejected",
        ]) {
            Self::HookDeclined
        } else if has(&["non-fast-forward", "fetch first", "updates were rejected"]) {
            Self::NonFastForward
        } else if has(&[
            "permission denied",
            "authentication failed",
            "access denied",
            "could not read username",
            "403",
        ]) {
            Self::Auth
        } else if has(&[
            "could not resolve host",
            "connection timed out",
            "connection refused",
            "unable to access",
            "could not read from remote repository",
            "network is unreachable",
        ]) {
            Self::Network
        } else {
            Self::Other
        }
    }
}

/// The git operations the toolkit needs. A [`GitCli`] in production, a fake in tests.
pub trait Vcs: Send + Sync + Debug {
    fn fetch(&self) -> Result<(), GitError>;
    fn merge_base(&self, a: &str, b: &str) -> Result<String, GitError>;
    /// Commits in `from..to`, oldest first; `first_parent` follows only the mainline.
    fn log(&self, from: &str, to: &str, first_parent: bool) -> Result<Vec<Commit>, GitError>;
    fn commit(&self, rev: &str) -> Result<Commit, GitError>;
    /// The latest `limit` commits of `rev`, newest first (`first_parent`: its own history only).
    fn recent(&self, rev: &str, limit: usize, first_parent: bool) -> Result<Vec<Commit>, GitError>;
    fn is_dirty(&self) -> Result<bool, GitError>;
    fn branch_exists(&self, name: &str) -> Result<bool, GitError>;
    /// Creates `name` at `start` (replacing an existing branch with `force`) and checks it out.
    fn create_branch(&self, name: &str, start: &str, force: bool) -> Result<(), GitError>;
    fn checkout(&self, name: &str) -> Result<(), GitError>;
    /// Reverts `hash` (a merge against its first parent), preferring the reverted side on conflicts.
    fn revert(&self, hash: &str) -> Result<Applied, GitError> {
        self.revert_with(hash, ApplyOptions::default())
    }
    /// Cherry-picks `hash`, preferring the picked side on conflicts and resolving modify/delete by deleting.
    fn cherry_pick(&self, hash: &str) -> Result<Applied, GitError> {
        self.cherry_pick_with(hash, ApplyOptions::default())
    }
    fn revert_with(&self, hash: &str, options: ApplyOptions) -> Result<Applied, GitError>;
    fn cherry_pick_with(&self, hash: &str, options: ApplyOptions) -> Result<Applied, GitError>;
    /// `merge --no-ff -m message rev` with the conflict handling of `options`.
    fn merge_with(
        &self,
        rev: &str,
        message: &str,
        options: ApplyOptions,
    ) -> Result<Applied, GitError>;
    /// The three versions of a path in the conflict in progress, each cut at `limit` bytes.
    fn conflict_versions(&self, path: &str, limit: usize) -> Result<ConflictVersions, GitError>;
    fn resolve_path(&self, path: &str, choice: Choice) -> Result<(), GitError>;
    /// Commits the operation in progress once every path is resolved (`Conflicted` when some are not).
    fn continue_op(&self, operation: Operation, commit: &str) -> Result<Applied, GitError>;
    /// Drops the change in progress (a revert or cherry-pick) and goes on without it.
    fn skip_op(&self, operation: Operation) -> Result<(), GitError>;
    /// Abandons the operation in progress; the work tree is as before it.
    fn abort_op(&self, operation: Operation) -> Result<(), GitError>;
    fn push(&self, branch: &str, mode: PushMode) -> Result<(), GitError>;
    /// Clones `url` into the (empty or missing) work tree, with every branch.
    fn clone_from(&self, url: &str) -> Result<(), GitError>;
    fn is_repository(&self) -> bool;
    /// `fetch --all --prune --force --tags`.
    fn fetch_all(&self) -> Result<(), GitError>;
    fn rev_parse(&self, rev: &str) -> Result<String, GitError>;
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, GitError>;
    /// Remote-tracking branches of origin under `prefix` (e.g. `release/`).
    fn remote_branches(&self, prefix: &str) -> Result<Vec<RemoteBranch>, GitError>;
    /// `checkout -B name start`: the branch is (re)set to `start` and checked out.
    fn checkout_reset(&self, name: &str, start: &str) -> Result<(), GitError>;
    fn reset_hard(&self, rev: &str) -> Result<(), GitError>;
    /// `merge --no-ff -m message rev`; a conflicting merge is aborted.
    fn merge_no_ff(&self, rev: &str, message: &str) -> Result<(), GitError>;
    /// The identity commits are made with: the one given to the client, else the repository's own config
    /// (`--local`: a global identity does not count, so a shared clone asks who is releasing).
    fn identity(&self) -> Result<Option<Identity>, GitError>;
    /// Stores the identity in the repository's own config.
    fn set_identity(&self, identity: &Identity) -> Result<(), GitError>;
    fn origin_url(&self) -> Result<Option<String>, GitError>;
    /// The checked-out branch; `None` when HEAD is detached.
    fn current_branch(&self) -> Result<Option<String>, GitError>;
    /// Detaches HEAD at `rev` (discarding local changes), so no branch is checked out.
    fn detach_at(&self, rev: &str) -> Result<(), GitError>;
    /// Deletes every local branch (HEAD must be detached); returns their names. Release work always starts
    /// from origin, so local branches only hold leftovers.
    fn delete_local_branches(&self) -> Result<Vec<String>, GitError>;
    /// `git cherry upstream head`: the commits of `head` not in `upstream`, each with whether an equivalent
    /// change (same patch-id) is in `upstream` anyway.
    fn cherry(&self, upstream: &str, head: &str) -> Result<Vec<(String, bool)>, GitError>;
    /// Where origin has `branch`, from the remote-tracking ref (`None`: origin does not have it).
    fn remote_head(&self, branch: &str) -> Result<Option<String>, GitError>;
    /// Mainline commits in `from..to` whose change (against their first parent) touches `path`.
    fn touching(&self, from: &str, to: &str, path: &str) -> Result<Vec<String>, GitError>;
    /// Writes `files` (path, content) into the work tree and commits them on the checked-out branch.
    /// `None`: they were already like that, nothing was committed.
    fn commit_files(
        &self,
        files: &[(String, String)],
        message: &str,
    ) -> Result<Option<String>, GitError>;
    /// A file's content at `rev` (`None` when it has no such file).
    fn show_file(&self, rev: &str, path: &str) -> Result<Option<String>, GitError>;
    /// The files that differ between two commits or trees, with line counts (renames detected).
    fn diff_files(&self, from: &str, to: &str) -> Result<Vec<DiffEntry>, GitError>;
    /// The unified diff of one path between two commits or trees (empty when it did not change).
    fn diff_text(&self, from: &str, to: &str, path: &str) -> Result<String, GitError>;
    /// Non-merge commits in `from..to`, oldest first, each with the paths it changed (against its parent).
    fn commits_touching(&self, from: &str, to: &str) -> Result<Vec<PathCommit>, GitError>;
    /// The tree `git merge` of `theirs` into `ours` would produce, without touching the work tree or any ref.
    fn merge_tree(&self, ours: &str, theirs: &str) -> Result<MergeTree, GitError>;
    /// The object id of the file at `rev` (`None` when there is no such file): it changes when the content does.
    fn blob_id(&self, rev: &str, path: &str) -> Result<Option<String>, GitError>;
    /// A file's bytes at `rev` (`None` when it has no such file), for binary files such as zips.
    fn show_bytes(&self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, GitError>;
    /// Writes, replaces or deletes files in the work tree and commits that on the checked-out branch.
    /// `None`: nothing changed, nothing was committed.
    fn commit_changes(
        &self,
        changes: &[FileChange],
        message: &str,
    ) -> Result<Option<String>, GitError>;
    /// Mainline commits in `from..to`, oldest first, each with the files it changed under `dir` (against its
    /// first parent). Commits that changed nothing there are left out.
    fn path_log(&self, from: &str, to: &str, dir: &str) -> Result<Vec<PathCommit>, GitError>;
    /// The files under `dir` at `rev`.
    fn files_at(&self, rev: &str, dir: &str) -> Result<Vec<String>, GitError>;
    /// `rebase --onto onto upstream` of the checked-out branch; a conflicting rebase is aborted.
    fn rebase_onto(&self, onto: &str, upstream: &str) -> Result<(), GitError>;
}
