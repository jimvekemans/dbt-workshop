//! An order-preserving YAML document model and an emitter in the layout of the Python utilities
//! (`yaml.dump(..., Dumper=DbtSourceDumper, default_flow_style=False, sort_keys=False)`: block style, sequences
//! indented under their key).

use crate::tree::{YNode, YValue};

#[derive(Debug, Clone, PartialEq)]
pub enum Doc {
    /// A scalar; `plain` remembers whether it was written unquoted.
    Scalar {
        text: String,
        plain: bool,
    },
    Seq(Vec<Doc>),
    Map(Vec<(String, Doc)>),
}

impl Doc {
    pub fn str(text: impl Into<String>) -> Self {
        let text = text.into();
        let plain = !needs_quotes(&text);
        Self::Scalar { text, plain }
    }

    pub fn map(entries: Vec<(&str, Self)>) -> Self {
        Self::Map(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        )
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Self> {
        match self {
            Self::Map(entries) => entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets a key, keeping its position when it exists.
    pub fn set(&mut self, key: &str, value: Self) {
        if let Self::Map(entries) = self {
            match entries.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => entries.push((key.to_owned(), value)),
            }
        }
    }

    pub fn items_mut(&mut self) -> Option<&mut Vec<Self>> {
        match self {
            Self::Seq(items) => Some(items),
            _ => None,
        }
    }

    /// The `name` of a mapping.
    pub fn name(&self) -> Option<&str> {
        self.get("name").and_then(Self::as_str)
    }

    /// Converts a parsed tree. Aliases are not supported in files the toolkit regenerates.
    pub fn from_node(node: &YNode) -> Result<Self, String> {
        Ok(match &node.value {
            YValue::Scalar { text, plain, .. } => Self::Scalar {
                text: text.clone(),
                plain: *plain,
            },
            YValue::Seq(items) => Self::Seq(
                items
                    .iter()
                    .map(Self::from_node)
                    .collect::<Result<_, _>>()?,
            ),
            YValue::Map(entries) => Self::Map(
                entries
                    .iter()
                    .map(|(k, v)| {
                        let key = k
                            .as_str()
                            .ok_or_else(|| format!("line {}: non-scalar key", k.start.line))?;
                        Ok((key.to_owned(), Self::from_node(v)?))
                    })
                    .collect::<Result<_, String>>()?,
            ),
            YValue::Alias(_) => {
                return Err(format!(
                    "line {}: anchors and aliases are not supported in generated source files",
                    node.start.line
                ));
            }
        })
    }

    /// The document as YAML text.
    pub fn emit(&self) -> String {
        let mut out = String::new();
        match self {
            Self::Map(entries) => emit_map(entries, 0, &mut out),
            other => {
                out.push_str(&inline(other).unwrap_or_default());
                out.push('\n');
            }
        }
        out
    }
}

fn pad(n: usize) -> String {
    " ".repeat(n)
}

/// A scalar or empty collection on one line, `None` for anything else.
fn inline(value: &Doc) -> Option<String> {
    match value {
        // A scalar that was plain in its file stays plain (it reads back the same); new ones were classified by
        // `Doc::str`.
        Doc::Scalar { text, plain } if !text.contains('\n') => {
            Some(if *plain { text.clone() } else { quote(text) })
        }
        Doc::Seq(items) if items.is_empty() => Some("[]".into()),
        Doc::Map(entries) if entries.is_empty() => Some("{}".into()),
        _ => None,
    }
}

fn emit_value_after_key(value: &Doc, indent: usize, out: &mut String) {
    if let Some(text) = inline(value) {
        out.push(' ');
        out.push_str(&text);
        out.push('\n');
        return;
    }
    match value {
        Doc::Scalar { text, .. } => emit_literal(text, indent + 2, out),
        Doc::Seq(items) => {
            out.push('\n');
            for item in items {
                emit_item(item, indent + 2, out);
            }
        }
        Doc::Map(entries) => {
            out.push('\n');
            emit_map(entries, indent + 2, out);
        }
    }
}

fn emit_map(entries: &[(String, Doc)], indent: usize, out: &mut String) {
    for (key, value) in entries {
        out.push_str(&pad(indent));
        out.push_str(&key_text(key));
        out.push(':');
        emit_value_after_key(value, indent, out);
    }
}

fn emit_item(item: &Doc, indent: usize, out: &mut String) {
    out.push_str(&pad(indent));
    out.push('-');
    if let Some(text) = inline(item) {
        out.push(' ');
        out.push_str(&text);
        out.push('\n');
        return;
    }
    match item {
        Doc::Map(entries) => {
            // The first entry shares the dash line.
            let (first_key, first_value) = &entries[0];
            out.push(' ');
            out.push_str(&key_text(first_key));
            out.push(':');
            emit_value_after_key(first_value, indent + 2, out);
            emit_map(&entries[1..], indent + 2, out);
        }
        Doc::Seq(items) => {
            out.push('\n');
            for nested in items {
                emit_item(nested, indent + 2, out);
            }
        }
        Doc::Scalar { text, .. } => emit_literal(text, indent + 2, out),
    }
}

fn emit_literal(text: &str, indent: usize, out: &mut String) {
    out.push_str(if text.ends_with('\n') {
        " |\n"
    } else {
        " |-\n"
    });
    for line in text.trim_end_matches('\n').split('\n') {
        if !line.is_empty() {
            out.push_str(&pad(indent));
            out.push_str(line);
        }
        out.push('\n');
    }
}

fn key_text(key: &str) -> String {
    if needs_quotes(key) {
        quote(key)
    } else {
        key.to_owned()
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Whether PyYAML (YAML 1.1 resolution) would quote this string: it would otherwise read back as something else
/// (a bool, null, a number) or not parse.
pub(crate) fn needs_quotes(text: &str) -> bool {
    if text.is_empty() || text.trim() != text || text.contains('\n') {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    if [
        "y", "n", "yes", "no", "on", "off", "true", "false", "null", "~", ".inf", "-.inf", "+.inf",
        ".nan",
    ]
    .contains(&lower.as_str())
    {
        return true;
    }
    if looks_numeric(text) {
        return true;
    }
    let first = text.chars().next().unwrap_or(' ');
    if "-?:,[]{}#&*!|>'\"%@`".contains(first) {
        // `-x` and `:x` are plain in YAML, but PyYAML quotes them; so do we.
        return true;
    }
    text.contains(": ") || text.contains(" #") || text.ends_with(':')
}

fn looks_numeric(text: &str) -> bool {
    let body = text.strip_prefix(['-', '+']).unwrap_or(text);
    if body.is_empty() {
        return false;
    }
    let digits_only = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '_');
    if let Some(hex) = body.strip_prefix("0x") {
        return !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit() || c == '_');
    }
    if digits_only(body) {
        return true;
    }
    // Floats: 1.5, .5, 1e3, 1.5e-3; sexagesimal 1:30.
    let (mantissa, exponent) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int, frac)) => {
            (int.is_empty() || digits_only(int))
                && (frac.is_empty() || digits_only(frac))
                && !(int.is_empty() && frac.is_empty())
        }
        None => digits_only(mantissa) && exponent.is_some(),
    };
    let exponent_ok = exponent.is_none_or(|e| digits_only(e.strip_prefix(['-', '+']).unwrap_or(e)));
    if mantissa_ok && exponent_ok {
        return true;
    }
    body.split(':').all(digits_only) && body.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_python_source_dumper() {
        let doc = Doc::map(vec![
            (
                "version",
                Doc::Scalar {
                    text: "2".into(),
                    plain: true,
                },
            ),
            (
                "sources",
                Doc::Seq(vec![Doc::map(vec![
                    ("name", Doc::str("edw_inc")),
                    ("database", Doc::str("edw_inc")),
                    ("schema", Doc::str("edw_inc")),
                    (
                        "tables",
                        Doc::Seq(vec![Doc::map(vec![
                            ("name", Doc::str("t1")),
                            (
                                "columns",
                                Doc::Seq(vec![
                                    Doc::map(vec![("name", Doc::str("id"))]),
                                    Doc::map(vec![("name", Doc::str("yes"))]),
                                ]),
                            ),
                        ])]),
                    ),
                ])]),
            ),
        ]);
        assert_eq!(
            doc.emit(),
            "version: 2\nsources:\n  - name: edw_inc\n    database: edw_inc\n    schema: edw_inc\n    tables:\n      - name: t1\n        columns:\n          - name: id\n          - name: 'yes'\n"
        );
    }

    #[test]
    fn quoting() {
        for plain in ["abc", "a_b", "Table1", "x-y", "a.b", "1abc"] {
            assert!(!needs_quotes(plain), "{plain}");
        }
        for quoted in [
            "", "yes", "No", "null", "123", "1.5", "0x1F", "1e3", "-a", "a: b", " a", "12:30",
        ] {
            assert!(needs_quotes(quoted), "{quoted}");
        }
    }

    #[test]
    fn round_trips_through_the_tree() {
        let text = "version: 2\nsources:\n  - name: s\n    description: |-\n      line one\n      line two\n    tables:\n      - name: 'yes'\n        columns: []\n";
        let doc = Doc::from_node(&crate::tree::parse(text).unwrap().unwrap()).unwrap();
        assert_eq!(doc.emit(), text);
    }
}
