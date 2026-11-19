//! `GitCli` against throw-away repositories: git's machine-readable output through the parsers, and conflict
//! detection for merge, cherry-pick and revert.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use vdp_git::{
    Applied, ApplyOptions, ConflictKind, ConflictPath, DiffEntry, GitCli, Identity, OnConflict,
    Operation, Strategy, Vcs,
};

struct Repo {
    dir: TempDir,
    git: GitCli,
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test Author")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "Test Author")
        .env("GIT_COMMITTER_EMAIL", "author@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

impl Repo {
    /// An empty repository on branch `main`, independent of the user's git configuration.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        git_in(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
        for (key, value) in [
            ("commit.gpgsign", "false"),
            ("merge.conflictStyle", "merge"),
            ("rerere.enabled", "false"),
            ("core.autocrlf", "false"),
            ("core.quotepath", "true"),
        ] {
            git_in(dir.path(), &["config", key, value]);
        }
        let git = GitCli::new(dir.path()).with_identity(Some(Identity {
            name: "Release Bot".into(),
            email: "bot@example.com".into(),
        }));
        Self { dir, git }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn remove(&self, relative: &str) {
        std::fs::remove_file(self.path(relative)).unwrap();
    }

    /// Stages everything and commits; returns the new commit.
    fn commit(&self, message: &str) -> String {
        git_in(self.dir.path(), &["add", "--all"]);
        git_in(self.dir.path(), &["commit", "--quiet", "-m", message]);
        self.git.rev_parse("HEAD").unwrap()
    }

    fn checkout_new(&self, branch: &str, start: &str) {
        git_in(
            self.dir.path(),
            &["checkout", "--quiet", "-b", branch, start],
        );
    }

    fn checkout(&self, branch: &str) {
        git_in(self.dir.path(), &["checkout", "--quiet", branch]);
    }
}

const PAUSE_STRICT: ApplyOptions = ApplyOptions {
    on_conflict: OnConflict::Pause,
    strategy: Strategy::Strict,
};

fn conflicted(applied: Applied) -> vdp_git::Conflict {
    match applied {
        Applied::Conflicted(conflict) => conflict,
        other => panic!("expected a conflict, got {other:?}"),
    }
}

fn path(path: &str, kind: ConflictKind) -> ConflictPath {
    ConflictPath {
        path: path.into(),
        kind,
    }
}

/// main: a.txt = base. feature: a.txt = feature. main moves on: a.txt = main.
fn diverged() -> (Repo, String) {
    let repo = Repo::new();
    repo.write("a.txt", "base\n");
    let base = repo.commit("base");
    repo.checkout_new("feature", &base);
    repo.write("a.txt", "feature\n");
    let feature = repo.commit("feature edits a.txt");
    repo.checkout("main");
    repo.write("a.txt", "main\n");
    repo.commit("main edits a.txt");
    (repo, feature)
}

#[test]
fn a_cherry_pick_of_a_change_to_the_same_lines_reports_one_content_conflict() {
    let (repo, feature) = diverged();

    let conflict = conflicted(repo.git.cherry_pick_with(&feature, PAUSE_STRICT).unwrap());

    assert_eq!(conflict.operation, Operation::CherryPick);
    assert_eq!(conflict.commit, feature);
    assert_eq!(conflict.paths, [path("a.txt", ConflictKind::Content)]);
    assert_eq!(
        repo.git.workspace_state().operation.as_deref(),
        Some("cherry-pick"),
        "the operation is left in progress for a decision"
    );
    repo.git.abort_op(Operation::CherryPick).unwrap();
    assert_eq!(repo.git.workspace_state().operation, None);
}

#[test]
fn a_revert_of_a_commit_that_later_work_built_on_reports_a_content_conflict() {
    let repo = Repo::new();
    repo.write("a.txt", "one\n");
    repo.commit("one");
    repo.write("a.txt", "two\n");
    let two = repo.commit("two");
    repo.write("a.txt", "three\n");
    repo.commit("three");

    let conflict = conflicted(repo.git.revert_with(&two, PAUSE_STRICT).unwrap());

    assert_eq!(conflict.operation, Operation::Revert);
    assert_eq!(conflict.commit, two);
    assert_eq!(conflict.paths, [path("a.txt", ConflictKind::Content)]);
    repo.git.abort_op(Operation::Revert).unwrap();
}

#[test]
fn a_merge_lists_every_conflicting_path_with_its_kind_sorted_by_path() {
    let repo = Repo::new();
    repo.write("content.txt", "base\n");
    repo.write("branch-deletes.txt", "base\n");
    repo.write("change-deletes.txt", "base\n");
    repo.write("untouched.txt", "base\n");
    let base = repo.commit("base");

    repo.checkout_new("feature", &base);
    repo.write("content.txt", "feature\n");
    repo.write("branch-deletes.txt", "feature\n");
    repo.remove("change-deletes.txt");
    repo.write("both added/naïve file.txt", "feature\n");
    let feature = repo.commit("feature");

    repo.checkout("main");
    repo.write("content.txt", "main\n");
    repo.remove("branch-deletes.txt");
    repo.write("change-deletes.txt", "main\n");
    repo.write("both added/naïve file.txt", "main\n");
    repo.commit("main");

    let conflict = conflicted(
        repo.git
            .merge_with(&feature, "Merge feature", PAUSE_STRICT)
            .unwrap(),
    );

    assert_eq!(conflict.operation, Operation::Merge);
    assert_eq!(conflict.commit, feature);
    assert_eq!(
        conflict.paths,
        [
            path("both added/naïve file.txt", ConflictKind::BothAdded),
            path("branch-deletes.txt", ConflictKind::BranchDeleted),
            path("change-deletes.txt", ConflictKind::ChangeDeletes),
            path("content.txt", ConflictKind::Content),
        ]
    );
    assert_eq!(
        repo.git.workspace_state().operation.as_deref(),
        Some("merge")
    );
    repo.git.abort_op(Operation::Merge).unwrap();
}

#[test]
fn a_change_that_applies_cleanly_is_committed_without_a_conflict() {
    let (repo, _feature) = diverged();
    repo.checkout_new("elsewhere", "main");
    repo.write("other.txt", "x\n");
    let other = repo.commit("other");
    repo.checkout("main");

    let applied = repo.git.cherry_pick_with(&other, PAUSE_STRICT).unwrap();

    assert_eq!(applied, Applied::Committed);
    assert_eq!(repo.git.workspace_state().operation, None);
}

#[test]
fn the_default_conflict_handling_resolves_for_the_change_and_names_the_paths() {
    let (repo, feature) = diverged();

    let applied = repo
        .git
        .cherry_pick_with(
            &feature,
            ApplyOptions {
                on_conflict: OnConflict::Theirs,
                strategy: Strategy::Strict,
            },
        )
        .unwrap();

    assert_eq!(applied, Applied::Resolved(vec!["a.txt".into()]));
    assert_eq!(
        std::fs::read_to_string(repo.path("a.txt")).unwrap(),
        "feature\n"
    );
}

#[test]
fn conflict_versions_show_base_branch_and_change() {
    let (repo, feature) = diverged();
    conflicted(repo.git.cherry_pick_with(&feature, PAUSE_STRICT).unwrap());

    let versions = repo.git.conflict_versions("a.txt", 1024).unwrap();

    assert_eq!(versions.base.as_deref(), Some("base\n"));
    assert_eq!(versions.branch.as_deref(), Some("main\n"));
    assert_eq!(versions.change.as_deref(), Some("feature\n"));
    repo.git.abort_op(Operation::CherryPick).unwrap();
}

fn entry(
    status: char,
    path: &str,
    old_path: Option<&str>,
    additions: Option<u64>,
    deletions: Option<u64>,
) -> DiffEntry {
    DiffEntry {
        status,
        path: path.into(),
        old_path: old_path.map(str::to_owned),
        additions,
        deletions,
    }
}

#[test]
fn diff_files_reads_real_git_output_with_renames_binaries_spaces_and_unicode() {
    let repo = Repo::new();
    repo.write("keep.sql", "one\ntwo\nthree\n");
    repo.write("old name.sql", "select 1\nfrom t\nwhere x\nand y\nand z\n");
    repo.write("gone.sql", "bye\n");
    std::fs::write(repo.path("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    let before = repo.commit("before");

    repo.write("keep.sql", "one\nTWO\nthree\nfour\n");
    std::fs::rename(repo.path("old name.sql"), repo.path("new nämé.sql")).unwrap();
    repo.remove("gone.sql");
    repo.write("dir/ünï code.sql", "a\nb\n");
    std::fs::write(repo.path("blob.bin"), [0u8, 9, 9, 9, 9]).unwrap();
    let after = repo.commit("after");

    let mut entries = repo.git.diff_files(&before, &after).unwrap();
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    assert_eq!(
        entries,
        [
            entry('M', "blob.bin", None, None, None),
            entry('A', "dir/ünï code.sql", None, Some(2), Some(0)),
            entry('D', "gone.sql", None, Some(0), Some(1)),
            entry('M', "keep.sql", None, Some(2), Some(1)),
            entry('R', "new nämé.sql", Some("old name.sql"), Some(0), Some(0)),
        ]
    );
}

#[test]
fn log_and_recent_read_real_git_output_including_a_merge_commit() {
    let repo = Repo::new();
    repo.write("a.txt", "base\n");
    let base = repo.commit("base\n\nwith a body, ünïcode ✓");
    repo.checkout_new("feature", &base);
    repo.write("f.txt", "f\n");
    let feature = repo.commit("feature work");
    repo.checkout("main");
    repo.write("m.txt", "m\n");
    let main = repo.commit("main work");
    let applied = repo
        .git
        .merge_with(&feature, "Merge branch 'feature/INTEL-1'", PAUSE_STRICT)
        .unwrap();
    assert_eq!(applied, Applied::Committed);
    let merge = repo.git.rev_parse("HEAD").unwrap();

    let log = repo.git.log(&base, "HEAD", false).unwrap();
    let hashes: Vec<&str> = log.iter().map(|c| c.hash.as_str()).collect();
    assert_eq!(hashes.len(), 3);
    assert_eq!(
        hashes.last(),
        Some(&merge.as_str()),
        "oldest first, merge last"
    );
    assert!(hashes.contains(&feature.as_str()) && hashes.contains(&main.as_str()));
    let merge_commit = log.last().unwrap();
    assert!(merge_commit.is_merge());
    assert_eq!(merge_commit.parents, [main.clone(), feature.clone()]);
    assert_eq!(merge_commit.subject(), "Merge branch 'feature/INTEL-1'");
    assert_eq!(merge_commit.author, "Release Bot");

    let first_parent = repo.git.log(&base, "HEAD", true).unwrap();
    assert_eq!(
        first_parent
            .iter()
            .map(|c| c.hash.clone())
            .collect::<Vec<_>>(),
        [main.clone(), merge.clone()]
    );

    let recent = repo.git.recent("HEAD", 10, true).unwrap();
    assert_eq!(recent.len(), 3, "merge, main work, base");
    assert_eq!(
        recent.last().unwrap().message,
        "base\n\nwith a body, ünïcode ✓"
    );
    assert_eq!(recent.last().unwrap().author, "Test Author");

    assert_eq!(repo.git.log(&merge, &merge, false).unwrap().len(), 0);
}

#[test]
fn commits_touching_pairs_each_commit_with_its_files_from_real_git_output() {
    let repo = Repo::new();
    repo.write("a.txt", "base\n");
    let base = repo.commit("base");
    repo.write("dir with space/one.sql", "1\n");
    repo.write("a.txt", "changed\n");
    let first = repo.commit("first");
    repo.remove("dir with space/one.sql");
    repo.write("two.sql", "2\n");
    let second = repo.commit("second");

    let touched = repo.git.commits_touching(&base, "HEAD").unwrap();

    assert_eq!(touched.len(), 2);
    assert_eq!(touched[0].commit.hash, first);
    let files = |i: usize| -> Vec<(char, &str)> {
        touched[i]
            .files
            .iter()
            .map(|f| (f.status, f.path.as_str()))
            .collect()
    };
    assert_eq!(files(0), [('M', "a.txt"), ('A', "dir with space/one.sql")]);
    assert_eq!(touched[1].commit.hash, second);
    assert_eq!(
        files(1),
        [('D', "dir with space/one.sql"), ('A', "two.sql")]
    );
}
