//! Adds a table to a hand-written source file without touching anything else.
//!
//! The file is not re-serialised: the new entry is inserted as text after the last table of the source, with
//! the indentation that file already uses, so comments, anchors, key order and quoting stay exactly as they
//! were. The result is parsed again and must equal the original plus the new table.

use std::fmt::Write as _;

use crate::doc::{Doc, needs_quotes};
use crate::tree::{self, Shape, YNode, YValue};

/// The table entry to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEntry {
    pub name: String,
    pub columns: Vec<String>,
}

impl TableEntry {
    /// The table as a document: its name and, with `include_columns` and when it has columns, the column names.
    pub fn to_doc(&self, include_columns: bool) -> Doc {
        let mut entries = vec![("name", Doc::str(self.name.clone()))];
        if include_columns && !self.columns.is_empty() {
            entries.push((
                "columns",
                Doc::Seq(
                    self.columns
                        .iter()
                        .map(|c| Doc::map(vec![("name", Doc::str(c.clone()))]))
                        .collect(),
                ),
            ));
        }
        Doc::map(entries)
    }
}

fn scalar(text: &str) -> String {
    if needs_quotes(text) {
        format!("'{}'", text.replace('\'', "''"))
    } else {
        text.to_owned()
    }
}

/// Inputs for [`insert_table`].
#[derive(Debug, Clone, Copy)]
pub struct InsertTable<'a> {
    pub source: &'a str,
    pub entry: &'a TableEntry,
    pub include_columns: bool,
}

/// `text` with `insert.entry` added to the tables of source `insert.source`.
///
/// # Errors
///
/// A message when `text` is not parseable YAML or is empty, has no `sources:` entry called `insert.source`, that entry has
/// no `tables:` list (add the first table by hand), or the layout of the first table cannot be located to copy it.
pub fn insert_table(text: &str, insert: InsertTable<'_>) -> Result<String, String> {
    let InsertTable {
        source,
        entry,
        include_columns,
    } = insert;
    let root = tree::parse(text)?.ok_or("the file is empty")?;
    let tables = find_tables(&root, source)?;
    let items = tables.items();
    let first = items.first().ok_or_else(|| {
        format!("source `{source}` has an empty `tables:` list; add the first table by hand")
    })?;

    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let first_line_no = match &first.value {
        YValue::Map(entries) => entries
            .first()
            .map_or(first.start.line, |(key, _)| key.start.line),
        _ => first.start.line,
    };
    let first_line = lines
        .get(first_line_no - 1)
        .ok_or("cannot locate the first table")?;
    if first_line.trim_start().starts_with('[') || matches!(tables_flow(text, tables), Some(true)) {
        return Err(format!(
            "source `{source}` lists its tables in flow style ([...]); add the table by hand"
        ));
    }
    // `      - name: customer`: the dash column and the key column.
    let chars: Vec<char> = first_line.chars().collect();
    // The column of the first key (a mapping's own marker sits on its `:` indicator).
    let key_col = match &first.value {
        YValue::Map(entries) => entries
            .first()
            .map(|(key, _)| key.start)
            .filter(|pos| pos.line == first.start.line)
            .map_or(first.start.col, |pos| pos.col),
        _ => first.start.col,
    };
    let dash_col = chars[..key_col.min(chars.len())]
        .iter()
        .rposition(|c| *c == '-')
        .ok_or("cannot find the `-` of the first table")?;

    // Insert after the last non-blank line of the last table that is not a comment at or left of the dash
    // (such comments belong to what follows).
    let last = items.last().unwrap_or(first);
    let end_line = tables.end.line.min(lines.len() + 1); // first line after the list (1-based)
    let mut insert_after = last.start.line; // 1-based
    for line_no in last.start.line..end_line {
        let line = lines[line_no - 1];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if trimmed.starts_with('#') && indent <= dash_col {
            continue;
        }
        insert_after = line_no;
    }

    let dash = " ".repeat(dash_col);
    let key = " ".repeat(key_col);
    let mut block = format!(
        "{dash}-{}name: {}{newline}",
        " ".repeat(key_col - dash_col - 1),
        scalar(&entry.name)
    );
    if include_columns && !entry.columns.is_empty() {
        let _ = write!(block, "{key}columns:{newline}");
        for column in &entry.columns {
            let _ = write!(block, "{key}  - name: {}{newline}", scalar(column));
        }
    }

    let mut out = String::with_capacity(text.len() + block.len());
    for (index, line) in lines.iter().enumerate() {
        out.push_str(line);
        if index + 1 == insert_after {
            if !line.ends_with('\n') {
                out.push_str(newline);
            }
            out.push_str(&block);
        }
    }
    verify(text, &out, source, entry, include_columns)?;
    Ok(out)
}

fn tables_flow(text: &str, tables: &YNode) -> Option<bool> {
    let line = text.lines().nth(tables.start.line.checked_sub(1)?)?;
    Some(line.chars().nth(tables.start.col)? == '[')
}

fn find_tables<'a>(root: &'a YNode, source: &str) -> Result<&'a YNode, String> {
    let sources = root.get("sources").ok_or("no `sources:` in this file")?;
    let decl = sources
        .items()
        .iter()
        .find(|s| s.get("name").and_then(YNode::as_str) == Some(source))
        .ok_or_else(|| format!("no source `{source}` in this file"))?;
    let tables = decl.get("tables").ok_or_else(|| {
        format!("source `{source}` has no `tables:`; add the first table by hand")
    })?;
    match tables.value {
        YValue::Seq(_) => Ok(tables),
        _ => Err(format!("`tables:` of source `{source}` is not a list")),
    }
}

/// The new text must parse, and equal the old one plus exactly the new table.
fn verify(
    before: &str,
    after: &str,
    source: &str,
    entry: &TableEntry,
    include_columns: bool,
) -> Result<(), String> {
    let old = tree::parse(before)?.ok_or("empty")?;
    let new = tree::parse(after)
        .map_err(|e| format!("the edited file does not parse ({e}); nothing was written"))?
        .ok_or("empty")?;
    let added = find_tables(&new, source)?
        .items()
        .last()
        .map(YNode::shape)
        .ok_or("the table did not land")?;
    let expected = shape_of(&entry.to_doc(include_columns));
    if added != expected {
        return Err(format!(
            "the table landed malformed ({added:?}); nothing was written"
        ));
    }
    let mut without = new.shape();
    remove_last_table(&mut without, source);
    if without != old.shape() {
        return Err("the edit would change other content; nothing was written".into());
    }
    Ok(())
}

fn shape_of(doc: &Doc) -> Shape {
    match doc {
        Doc::Scalar { text, .. } => Shape::Scalar(text.clone()),
        Doc::Seq(items) => Shape::Seq(items.iter().map(shape_of).collect()),
        Doc::Map(entries) => Shape::Map(
            entries
                .iter()
                .map(|(k, v)| (Shape::Scalar(k.clone()), shape_of(v)))
                .collect(),
        ),
    }
}

fn remove_last_table(shape: &mut Shape, source: &str) {
    let Shape::Map(root) = shape else { return };
    let Some((_, Shape::Seq(sources))) = root
        .iter_mut()
        .find(|(k, _)| *k == Shape::Scalar("sources".into()))
    else {
        return;
    };
    for decl in sources {
        let Shape::Map(entries) = decl else { continue };
        let is_source = entries
            .iter()
            .any(|(k, v)| *k == Shape::Scalar("name".into()) && *v == Shape::Scalar(source.into()));
        if !is_source {
            continue;
        }
        if let Some((_, Shape::Seq(tables))) = entries
            .iter_mut()
            .find(|(k, _)| *k == Shape::Scalar("tables".into()))
        {
            tables.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAND_WRITTEN: &str = "\
anchors:
  - &hourly
    warn_after: {count: 1, period: hour}

sources:
  - name: tpch_raw   # the raw layer
    database: tpch_raw
    config:
      freshness: *hourly
    tables:
      - name: customer
        config:
          freshness: null
      - name: probe
        config:
          freshness:
            filter: is_live = 1
  # unrelated trailing comment
  - name: other
    tables:
    -   name: x
";

    fn entry(name: &str, columns: &[&str]) -> TableEntry {
        TableEntry {
            name: name.into(),
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    #[test]
    fn inserts_after_the_last_table_with_the_files_own_indentation() {
        let out = insert_table(
            HAND_WRITTEN,
            InsertTable {
                source: "tpch_raw",
                entry: &entry("new_t", &["id", "yes"]),
                include_columns: true,
            },
        )
        .unwrap();
        let expected = HAND_WRITTEN.replace(
            "            filter: is_live = 1\n",
            "            filter: is_live = 1\n      - name: new_t\n        columns:\n          - name: id\n          - name: 'yes'\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn follows_unusual_indentation() {
        let out = insert_table(
            HAND_WRITTEN,
            InsertTable {
                source: "other",
                entry: &entry("y", &[]),
                include_columns: false,
            },
        )
        .unwrap();
        // `y` is a YAML 1.1 boolean, so it is quoted.
        assert!(
            out.ends_with("    -   name: x\n    -   name: 'y'\n"),
            "{out}"
        );
    }

    #[test]
    fn refuses_what_it_cannot_edit_safely() {
        let flow = "sources:\n  - name: s\n    tables: [{name: a}]\n";
        assert!(
            insert_table(
                flow,
                InsertTable {
                    source: "s",
                    entry: &entry("b", &[]),
                    include_columns: false,
                },
            )
            .unwrap_err()
            .contains("flow")
        );
        assert!(
            insert_table(
                HAND_WRITTEN,
                InsertTable {
                    source: "nope",
                    entry: &entry("b", &[]),
                    include_columns: false,
                },
            )
            .is_err()
        );
    }

    #[test]
    fn handles_a_file_without_final_newline() {
        let text = "sources:\n  - name: s\n    tables:\n      - name: a";
        let out = insert_table(
            text,
            InsertTable {
                source: "s",
                entry: &entry("b", &[]),
                include_columns: false,
            },
        )
        .unwrap();
        assert_eq!(
            out,
            "sources:\n  - name: s\n    tables:\n      - name: a\n      - name: b\n"
        );
    }
}
