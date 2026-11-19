/// Replacement of the byte range `start..end` (an insertion when equal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl TextEdit {
    /// An edit that inserts `text` at byte offset `at`.
    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self {
            start: at,
            end: at,
            text: text.into(),
        }
    }

    /// Applies non-overlapping edits; returns `None` when edits overlap.
    pub fn apply_all(source: &str, mut edits: Vec<Self>) -> Option<String> {
        edits.sort_by_key(|e| (e.start, e.end));
        if edits.windows(2).any(|w| w[0].end > w[1].start) {
            return None;
        }
        let mut out =
            String::with_capacity(source.len() + edits.iter().map(|e| e.text.len()).sum::<usize>());
        let mut cursor = 0;
        for edit in &edits {
            out.push_str(&source[cursor..edit.start]);
            out.push_str(&edit.text);
            cursor = edit.end;
        }
        out.push_str(&source[cursor..]);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_in_position_order_and_rejects_overlaps() {
        let edits = vec![TextEdit::insert(5, "!"), TextEdit::insert(0, ">")];
        assert_eq!(
            TextEdit::apply_all("hello world", edits).unwrap(),
            ">hello! world"
        );
        let overlapping = vec![
            TextEdit {
                start: 0,
                end: 4,
                text: String::new(),
            },
            TextEdit {
                start: 2,
                end: 6,
                text: String::new(),
            },
        ];
        assert!(TextEdit::apply_all("hello world", overlapping).is_none());
    }
}
