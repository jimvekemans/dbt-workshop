use tree_sitter::Node;

use super::DagTransformer;
use crate::document::string_content;
use crate::{DagContext, DagDocument, DagError, PyLiteral, TextEdit};

/// Ensures `default_args = {...}` contains `"key": value`.
///
/// Presence is checked by dictionary key, never by substring (the Python version skipped
/// the pool whenever the word "pool" appeared anywhere in the file). New entries are
/// appended after the last entry with the dictionary's own indentation, adding the
/// separating comma when the last entry has none.
#[derive(Debug, Clone)]
pub struct DefaultArgsEntry {
    id: String,
    key: String,
    value: PyLiteral,
}

impl DefaultArgsEntry {
    /// Ensures that `default_args` has `key` set to `value`.
    pub fn new(key: impl Into<String>, value: PyLiteral) -> Self {
        let key = key.into();
        Self {
            id: format!("default-args:{key}"),
            key,
            value,
        }
    }
}

impl DagTransformer for DefaultArgsEntry {
    fn id(&self) -> &str {
        &self.id
    }

    fn edits(&self, doc: &DagDocument, _: &DagContext) -> Result<Vec<TextEdit>, DagError> {
        let Some(dict) = default_args_dict(doc) else {
            return Err(DagError::NotApplicable {
                dag: doc.name().to_owned(),
                transformer: self.id.clone(),
                reason: "no `default_args = {...}` dictionary found".into(),
            });
        };
        let mut cursor = dict.walk();
        let pairs: Vec<Node<'_>> = dict
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "pair")
            .collect();
        let present = pairs.iter().any(|pair| {
            pair.child_by_field_name("key")
                .and_then(|key| string_content(doc, key))
                .is_some_and(|key| key == self.key)
        });
        if present {
            return Ok(Vec::new());
        }

        let entry = format!("\"{}\": {}", self.key, self.value);
        let Some(last) = pairs.last() else {
            return Ok(vec![TextEdit::insert(
                dict.start_byte() + 1,
                format!("\n\t{entry},\n"),
            )]);
        };
        let text = doc.text();
        let indent = &text[dict.start_byte() + 1..pairs[0].start_byte()];
        let tail = &text[last.end_byte()..dict.end_byte() - 1];
        let edit = match tail.find(',') {
            Some(comma) => {
                TextEdit::insert(last.end_byte() + comma + 1, format!("{indent}{entry},"))
            }
            None => TextEdit::insert(last.end_byte(), format!(",{indent}{entry},")),
        };
        Ok(vec![edit])
    }
}

fn default_args_dict(doc: &DagDocument) -> Option<Node<'_>> {
    doc.descendants().into_iter().find_map(|node| {
        if node.kind() != "assignment" {
            return None;
        }
        let left = node.child_by_field_name("left")?;
        let right = node.child_by_field_name("right")?;
        (doc.node_text(left) == "default_args" && right.kind() == "dictionary").then_some(right)
    })
}
