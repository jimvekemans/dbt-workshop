//! Help texts and module documentation say what a command does, not which Python script of the old utilities it replaces:
//! no Rust source of the workspace names such a script by its file name.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Whether `line` ends a word with the extension of a Python file (`name` plus the extension, not `.pyi` or `.pyc`).
fn names_a_script(line: &str, extension: &str) -> bool {
    line.match_indices(extension).any(|(at, _)| {
        let after = line[at + extension.len()..].chars().next();
        !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && line[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

#[test]
fn no_source_names_a_python_script() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    rust_files(&crates, &mut files);
    assert!(files.len() > 50, "found only {} sources", files.len());
    let extension = concat!(".", "py");
    let mut hits = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for (n, line) in text.lines().enumerate() {
            if names_a_script(line, extension) {
                hits.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(hits.is_empty(), "Python script names:\n{}", hits.join("\n"));
}

#[test]
fn a_script_name_is_told_from_other_text() {
    let ext = concat!(".", "py");
    assert!(names_a_script(&format!("(`update_ticket{ext}`)"), ext));
    assert!(names_a_script(&format!("see call_sql{ext}."), ext));
    assert!(!names_a_script(&format!("the {ext} files"), ext));
    assert!(!names_a_script(&format!("x{ext}i"), ext));
    assert!(!names_a_script("no script here", ext));
}
