//! The VaultSpeed side of the change overview: plain files as for dbt, zips entry by entry (a zip is binary, so
//! git only says that it changed).

use std::path::PathBuf;

use similar::TextDiff;
use vdp_git::Vcs;
use vdp_release::changes::{ArchiveEntry, FileDiff, file_diff};
use vdp_vaultspeed::sources::{ArtifactReader, SourceFile, ZipArchiveReader};

use crate::VsError;

fn is_zip(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

fn entries(vcs: &dyn Vcs, rev: &str, path: &str) -> Result<Vec<SourceFile>, VsError> {
    match vcs.show_bytes(rev, path)? {
        None => Ok(Vec::new()),
        Some(bytes) => ZipArchiveReader::from_bytes(PathBuf::from(path), bytes)
            .read()
            .map_err(|e| VsError::Zip {
                name: path.to_owned(),
                detail: e.to_string(),
            }),
    }
}

fn size(file: &SourceFile) -> u64 {
    u64::try_from(file.content.len()).unwrap_or(u64::MAX)
}

/// One file's change; for a zip, its entries compared (added, deleted, modified, unchanged).
pub fn file_change(
    vcs: &dyn Vcs,
    from: &str,
    to: &str,
    path: &str,
    limit: usize,
) -> Result<FileDiff, VsError> {
    if !is_zip(path) {
        return Ok(file_diff(vcs, from, to, path, limit)?);
    }
    let old = entries(vcs, from, path)?;
    let new = entries(vcs, to, path)?;
    let mut compared: Vec<ArchiveEntry> = new
        .iter()
        .map(|file| {
            let name = file.path.to_string();
            let before = old.iter().find(|o| o.path == file.path);
            ArchiveEntry {
                status: match before {
                    None => "added",
                    Some(o) if o.content == file.content => "unchanged",
                    Some(_) => "modified",
                }
                .into(),
                old_size: before.map(size),
                new_size: Some(size(file)),
                name,
            }
        })
        .collect();
    compared.extend(
        old.iter()
            .filter(|o| !new.iter().any(|n| n.path == o.path))
            .map(|o| ArchiveEntry {
                name: o.path.to_string(),
                status: "deleted".into(),
                old_size: Some(size(o)),
                new_size: None,
            }),
    );
    compared.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(FileDiff {
        path: path.to_owned(),
        diff: String::new(),
        truncated: false,
        binary: true,
        entries: compared,
    })
}

/// The unified diff of one entry of a zip between the two states.
pub fn entry_change(
    vcs: &dyn Vcs,
    from: &str,
    to: &str,
    path: &str,
    entry: &str,
    limit: usize,
) -> Result<FileDiff, VsError> {
    let text = |files: &[SourceFile]| {
        files
            .iter()
            .find(|f| f.path.to_string() == entry)
            .map(|f| f.content.clone())
            .unwrap_or_default()
    };
    let old = text(&entries(vcs, from, path)?);
    let new = text(&entries(vcs, to, path)?);
    let mut diff = TextDiff::from_lines(&old, &new)
        .unified_diff()
        .header(&format!("a/{entry}"), &format!("b/{entry}"))
        .to_string();
    let truncated = diff.len() > limit;
    if truncated {
        let mut cut = limit;
        while !diff.is_char_boundary(cut) {
            cut -= 1;
        }
        diff.truncate(cut);
    }
    Ok(FileDiff {
        path: format!("{path}:{entry}"),
        diff,
        truncated,
        binary: false,
        entries: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;
    use std::process::Command;

    use vdp_git::GitCli;
    use vdp_release::changes::file_changes;

    use super::*;

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            for (name, content) in files {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(content.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// A zip that VaultSpeed delivered again: entries compared one by one, each with its own diff.
    #[test]
    fn a_changed_zip_is_compared_entry_by_entry() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q", "-b", "main"]);
        let zip = "vaultspeed/staging/release/700_DDL.zip";
        std::fs::create_dir_all(repo.join("vaultspeed/staging/release")).unwrap();
        std::fs::write(
            repo.join(zip),
            zip_with(&[
                ("DDL/a.sql", "CREATE TABLE a (id INT);\n"),
                ("DDL/b.sql", "DROP TABLE b;\n"),
                ("info.txt", "same\n"),
            ]),
        )
        .unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "Added 700_DDL"]);
        let from = git(repo, &["rev-parse", "HEAD"]);
        std::fs::write(
            repo.join(zip),
            zip_with(&[
                ("DDL/a.sql", "CREATE TABLE a (id BIGINT);\n"),
                ("DDL/c.sql", "CREATE TABLE c (id INT);\n"),
                ("info.txt", "same\n"),
            ]),
        )
        .unwrap();
        git(repo, &["commit", "-q", "-am", "Updated 700_DDL"]);
        let to = git(repo, &["rev-parse", "HEAD"]);
        let vcs = GitCli::new(repo);

        let files = file_changes(&vcs, &from, &to, None).unwrap();
        assert_eq!(files.files.len(), 1);
        assert!(files.files[0].binary, "git only says the zip changed");

        let change = file_change(&vcs, &from, &to, zip, 1 << 20).unwrap();
        let by_name: Vec<(&str, &str)> = change
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.status.as_str()))
            .collect();
        assert_eq!(
            by_name,
            [
                ("DDL/a.sql", "modified"),
                ("DDL/b.sql", "deleted"),
                ("DDL/c.sql", "added"),
                ("info.txt", "unchanged"),
            ]
        );
        assert_eq!(change.entries[1].new_size, None);
        assert_eq!(change.entries[1].old_size, Some(14));

        let entry = entry_change(&vcs, &from, &to, zip, "DDL/a.sql", 1 << 20).unwrap();
        assert!(
            entry.diff.contains("-CREATE TABLE a (id INT);"),
            "{}",
            entry.diff
        );
        assert!(
            entry.diff.contains("+CREATE TABLE a (id BIGINT);"),
            "{}",
            entry.diff
        );
        let deleted = entry_change(&vcs, &from, &to, zip, "DDL/b.sql", 1 << 20).unwrap();
        assert!(deleted.diff.contains("-DROP TABLE b;"), "{}", deleted.diff);
        let cut = entry_change(&vcs, &from, &to, zip, "DDL/a.sql", 10).unwrap();
        assert!(cut.truncated && cut.diff.len() <= 10);

        // A file that is not a zip gets git's own diff.
        std::fs::write(repo.join("pml.sql"), "select 1;\n").unwrap();
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "PML"]);
        let pml = git(repo, &["rev-parse", "HEAD"]);
        let plain = file_change(&vcs, &to, &pml, "pml.sql", 1 << 20).unwrap();
        assert!(plain.entries.is_empty());
        assert!(plain.diff.contains("+select 1;"), "{}", plain.diff);
    }
}
