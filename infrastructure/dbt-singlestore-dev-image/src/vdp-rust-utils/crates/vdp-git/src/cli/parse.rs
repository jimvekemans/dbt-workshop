//! Parsers for `git log` and `git diff` machine-readable outputs.

use crate::{ChangedPath, Commit, DiffEntry, GitError};

pub(super) const FIELD: char = '\u{1f}';
pub(super) const RECORD: char = '\u{1e}';
pub(super) const FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ct%x1f%B%x1e";

pub(super) fn parse_log(text: &str) -> Result<Vec<Commit>, GitError> {
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
pub(super) fn parse_name_status(text: &str) -> Vec<DiffEntry> {
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
pub(super) fn parse_numstat(text: &str) -> Vec<(String, Option<u64>, Option<u64>)> {
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
pub(super) fn split_name_status(chunk: &str) -> (Vec<ChangedPath>, &str) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hash: &str, parents: &str, author: &str, time: &str, message: &str) -> String {
        format!("{hash}{FIELD}{parents}{FIELD}{author}{FIELD}{time}{FIELD}{message}{RECORD}")
    }

    #[test]
    fn log_of_nothing_is_empty() {
        assert_eq!(parse_log("").unwrap(), []);
        assert_eq!(parse_log("\n").unwrap(), []);
        assert_eq!(parse_log(&RECORD.to_string()).unwrap(), []);
    }

    #[test]
    fn log_record_fields_land_in_the_right_place() {
        let text = record(
            "a1",
            "p1",
            "Jane Doe",
            "1700000000",
            "subject\n\nbody line\n",
        );
        assert_eq!(
            parse_log(&text).unwrap(),
            [Commit {
                hash: "a1".into(),
                parents: vec!["p1".into()],
                author: "Jane Doe".into(),
                timestamp: 1_700_000_000,
                message: "subject\n\nbody line".into(),
            }]
        );
    }

    #[test]
    fn log_root_and_merge_commits_have_zero_and_two_parents() {
        let text = format!(
            "{}\n{}",
            record("r0", "", "Jane", "1", "root"),
            record("m2", "p1 p2", "Jane", "2", "Merge branch 'x'")
        );
        let commits = parse_log(&text).unwrap();
        assert_eq!(commits[0].parents, Vec::<String>::new());
        assert!(!commits[0].is_merge());
        assert_eq!(commits[1].parents, ["p1", "p2"]);
        assert!(commits[1].is_merge());
    }

    #[test]
    fn log_keeps_unicode_authors_and_messages() {
        let text = record(
            "a1",
            "p1",
            "Zoë Müller-Łukasiewicz",
            "1700000000",
            "fix: naïve café \u{1f680} ünïcode",
        );
        let commit = parse_log(&text).unwrap().pop().unwrap();
        assert_eq!(commit.author, "Zoë Müller-Łukasiewicz");
        assert_eq!(commit.message, "fix: naïve café \u{1f680} ünïcode");
    }

    #[test]
    fn log_message_may_contain_the_field_separator() {
        let text = record("a1", "p1", "Jane", "5", &format!("one{FIELD}two"));
        assert_eq!(
            parse_log(&text).unwrap()[0].message,
            format!("one{FIELD}two")
        );
    }

    #[test]
    fn log_rejects_a_record_with_missing_fields() {
        let text = format!("a1{FIELD}p1{FIELD}Jane{RECORD}");
        let Err(GitError::Parse(detail)) = parse_log(&text) else {
            panic!("expected a parse error");
        };
        assert!(detail.starts_with("a1"), "{detail:?}");
    }

    #[test]
    fn log_rejects_a_timestamp_that_is_not_a_number() {
        let text = record("a1", "p1", "Jane", "yesterday", "msg");
        let Err(GitError::Parse(detail)) = parse_log(&text) else {
            panic!("expected a parse error");
        };
        assert_eq!(detail, "yesterday");
    }

    #[test]
    fn name_status_of_nothing_is_empty() {
        assert_eq!(parse_name_status(""), []);
        assert_eq!(parse_name_status("\0"), []);
    }

    fn entry(status: char, path: &str, old_path: Option<&str>) -> DiffEntry {
        DiffEntry {
            status,
            path: path.into(),
            old_path: old_path.map(str::to_owned),
            additions: None,
            deletions: None,
        }
    }

    #[test]
    fn name_status_reads_added_modified_deleted_and_type_changes() {
        let text = "A\0new.sql\0M\0models/a.sql\0D\0gone.sql\0T\0link\0";
        assert_eq!(
            parse_name_status(text),
            [
                entry('A', "new.sql", None),
                entry('M', "models/a.sql", None),
                entry('D', "gone.sql", None),
                entry('T', "link", None),
            ]
        );
    }

    #[test]
    fn name_status_renames_and_copies_carry_the_old_path_and_swallow_the_score() {
        let text = "R100\0old name.sql\0new name.sql\0C75\0src.sql\0copy.sql\0M\0after.sql\0";
        assert_eq!(
            parse_name_status(text),
            [
                entry('R', "new name.sql", Some("old name.sql")),
                entry('C', "copy.sql", Some("src.sql")),
                entry('M', "after.sql", None),
            ]
        );
    }

    #[test]
    fn name_status_keeps_unicode_and_whitespace_paths_as_they_are() {
        let text = "A\0dir/naïve café/ünï code.sql\0M\0 leading and trailing \0";
        assert_eq!(
            parse_name_status(text),
            [
                entry('A', "dir/naïve café/ünï code.sql", None),
                entry('M', " leading and trailing ", None),
            ]
        );
    }

    #[test]
    fn name_status_drops_a_status_without_a_path() {
        assert_eq!(
            parse_name_status("M\0a.sql\0D\0"),
            [entry('M', "a.sql", None)]
        );
    }

    #[test]
    fn numstat_reads_counts_and_paths_including_binary_files() {
        let text = "3\t1\tmodels/a.sql\0-\t-\timage.png\0";
        assert_eq!(
            parse_numstat(text),
            [
                ("models/a.sql".to_owned(), Some(3), Some(1)),
                ("image.png".to_owned(), None, None),
            ]
        );
    }

    #[test]
    fn numstat_rename_takes_the_new_path_from_the_second_following_field() {
        let text = concat!("0\t0\t\0old name.sql\0new name.sql\0", "7\t2\tafter.sql\0");
        assert_eq!(
            parse_numstat(text),
            [
                ("new name.sql".to_owned(), Some(0), Some(0)),
                ("after.sql".to_owned(), Some(7), Some(2)),
            ]
        );
    }

    #[test]
    fn numstat_keeps_tabs_inside_a_path_and_unicode() {
        let text = "1\t0\tdir/naïve\tfile é.sql\0";
        assert_eq!(
            parse_numstat(text),
            [("dir/naïve\tfile é.sql".to_owned(), Some(1), Some(0))]
        );
    }

    #[test]
    fn numstat_of_nothing_is_empty() {
        assert_eq!(parse_numstat(""), []);
        assert_eq!(parse_numstat("\0\0"), []);
    }

    fn changed(status: char, path: &str) -> ChangedPath {
        ChangedPath {
            status,
            path: path.into(),
        }
    }

    #[test]
    fn name_status_lines_stop_where_the_next_log_record_starts() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let chunk = format!(
            "\n\nM\ta.sql\nA\tdir/with space/ünï.sql\nR087\told.sql\tnew.sql\n\n{hash}{FIELD}p1{FIELD}Jane{FIELD}9{FIELD}msg"
        );
        let (files, rest) = split_name_status(&chunk);
        assert_eq!(
            files,
            [
                changed('M', "a.sql"),
                changed('A', "dir/with space/ünï.sql"),
                changed('R', "new.sql"),
            ]
        );
        assert_eq!(
            rest,
            format!("{hash}{FIELD}p1{FIELD}Jane{FIELD}9{FIELD}msg")
        );
    }

    #[test]
    fn name_status_lines_of_an_empty_chunk_are_empty() {
        assert_eq!(split_name_status(""), (vec![], ""));
        assert_eq!(split_name_status("\n\n"), (vec![], ""));
    }

    #[test]
    fn name_status_lines_reject_unknown_statuses_and_pathless_lines() {
        let (files, rest) = split_name_status("X\tweird.sql\nM\ta.sql\n");
        assert_eq!(files, []);
        assert_eq!(rest, "X\tweird.sql\nM\ta.sql\n");

        let (files, rest) = split_name_status("M\n");
        assert_eq!(files, []);
        assert_eq!(rest, "M\n");
    }

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
