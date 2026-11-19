//! Which files a deploy changes, and how: the overview the release UI shows before (or after) a promotion,
//! release, patch or VaultSpeed merge. Shared by the dbt and the VaultSpeed side.
//!
//! `from` and `to` are commits or trees (a promotion's result is a tree from `git merge-tree`, computed without
//! touching the work tree). The commits that touched each file (with their authors) come from a separate commit
//! range, because a tree has no history.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use vdp_git::{DiffEntry, GitError, Vcs};

use crate::analysis::CommitInfo;

/// At most this many commits are listed per file (the newest).
pub const COMMITS_PER_FILE: usize = 25;

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangedFile {
    pub path: String,
    /// The path before a rename or copy.
    pub old_path: Option<String>,
    /// `added`, `modified`, `deleted`, `renamed`, `copied` or `type_changed`.
    pub status: String,
    /// Lines added and removed; `None` for a binary file (a zip, an image).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub additions: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub deletions: Option<u64>,
    pub binary: bool,
    /// Distinct authors of the commits that touched it, most commits first (with history only).
    pub authors: Vec<String>,
    /// Those commits, newest first (at most [`COMMITS_PER_FILE`]).
    pub commits: Vec<CommitInfo>,
}

/// The files between two states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileChanges {
    pub from: String,
    pub to: String,
    pub files: Vec<ChangedFile>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub additions: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub deletions: u64,
    /// Whether `authors`/`commits` were filled in.
    pub with_history: bool,
    /// Paths the merge would leave conflicting (a patch promotion).
    pub conflicts: Vec<String>,
}

/// One entry of an archive (a VaultSpeed zip) compared between the two states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ArchiveEntry {
    pub name: String,
    /// `added`, `deleted`, `modified` or `unchanged`.
    pub status: String,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub old_size: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub new_size: Option<u64>,
}

/// One file's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileDiff {
    pub path: String,
    /// A unified diff (empty for a binary file).
    pub diff: String,
    /// The diff was cut at the size limit.
    pub truncated: bool,
    pub binary: bool,
    /// For an archive: its entries compared (empty otherwise).
    pub entries: Vec<ArchiveEntry>,
}

fn status_name(status: char) -> &'static str {
    match status {
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'C' => "copied",
        'T' => "type_changed",
        _ => "modified",
    }
}

/// Inputs for [`file_changes`].
#[derive(Debug, Clone, Copy)]
pub struct ChangesRequest<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub history: Option<(&'a str, &'a str)>,
}

/// Inputs for [`file_diff`].
#[derive(Debug, Clone, Copy)]
pub struct FileDiffRequest<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub path: &'a str,
    pub limit: usize,
}

/// The files that differ between `from` and `to`; with `history` (a commit range), the commits that touched
/// each file and their authors.
///
/// # Errors
///
/// [`GitError`] when the diff or (with `history`) the commit listing fails.
pub fn file_changes(vcs: &dyn Vcs, request: &ChangesRequest<'_>) -> Result<FileChanges, GitError> {
    let entries: Vec<DiffEntry> = vcs.diff_files(request.from, request.to)?;
    let mut by_path: BTreeMap<String, Vec<CommitInfo>> = BTreeMap::new();
    if let Some((start, end)) = request.history {
        for touched in vcs.commits_touching(start, end)? {
            let info = CommitInfo::from_commit(&touched.commit, None);
            for file in touched.files {
                by_path.entry(file.path).or_default().push(info.clone());
            }
        }
    }
    let mut changes = FileChanges {
        from: request.from.to_owned(),
        to: request.to.to_owned(),
        files: Vec::new(),
        additions: 0,
        deletions: 0,
        with_history: request.history.is_some(),
        conflicts: Vec::new(),
    };
    for entry in entries {
        let mut commits: Vec<CommitInfo> = by_path.get(&entry.path).cloned().unwrap_or_default();
        if let Some(old) = &entry.old_path {
            commits.extend(by_path.get(old).cloned().unwrap_or_default());
        }
        commits.sort_by_key(|c| std::cmp::Reverse(c.timestamp));
        commits.dedup_by(|a, b| a.hash == b.hash);
        let mut counts: Vec<(String, usize)> = Vec::new();
        for commit in &commits {
            match counts.iter_mut().find(|(name, _)| *name == commit.author) {
                Some((_, n)) => *n += 1,
                None => counts.push((commit.author.clone(), 1)),
            }
        }
        counts.sort_by_key(|c| std::cmp::Reverse(c.1));
        commits.truncate(COMMITS_PER_FILE);
        changes.additions += entry.additions.unwrap_or(0);
        changes.deletions += entry.deletions.unwrap_or(0);
        changes.files.push(ChangedFile {
            binary: entry.additions.is_none(),
            status: status_name(entry.status).to_owned(),
            path: entry.path,
            old_path: entry.old_path,
            additions: entry.additions,
            deletions: entry.deletions,
            authors: counts.into_iter().map(|(name, _)| name).collect(),
            commits,
        });
    }
    Ok(changes)
}

/// One file's unified diff, cut at `request.limit` bytes.
///
/// # Errors
///
/// [`GitError`] when the diff fails.
pub fn file_diff(vcs: &dyn Vcs, request: &FileDiffRequest<'_>) -> Result<FileDiff, GitError> {
    let mut diff = vcs.diff_text(request.from, request.to, request.path)?;
    let binary = diff
        .lines()
        .any(|l| l.starts_with("Binary files ") && l.ends_with(" differ"));
    let truncated = diff.len() > request.limit;
    if truncated {
        let mut cut = request.limit;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
    }
    Ok(FileDiff {
        path: request.path.to_owned(),
        diff: if binary { String::new() } else { diff },
        truncated,
        binary,
        entries: Vec::new(),
    })
}

/// Whether `value` may be passed to git as one side of a comparison: a full object id, or a remote-tracking
/// branch (`origin/<name>`) without `..` or options.
pub fn is_comparable_ref(value: &str) -> bool {
    let id = value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit());
    let branch = value.strip_prefix("origin/").is_some_and(|name| {
        !name.is_empty()
            && !name.contains("..")
            && !name.starts_with('-')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
    });
    id || branch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparable_refs() {
        assert!(is_comparable_ref("origin/release/2026_DROP25"));
        assert!(is_comparable_ref(&"a".repeat(40)));
        for bad in [
            "main",
            "origin/a..b",
            "origin/-x",
            "HEAD~1",
            "origin/",
            "--output=x",
            "abc",
        ] {
            assert!(!is_comparable_ref(bad), "{bad}");
        }
    }
}
