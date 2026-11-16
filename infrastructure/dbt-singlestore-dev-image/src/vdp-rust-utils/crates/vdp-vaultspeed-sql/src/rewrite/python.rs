//! Translation of Python `re` syntax used in `QUERY_MODIFICATIONS.json` to Rust `regex`.

use std::fmt::Write as _;

/// Converts a Python `re.sub` replacement template to `regex` expansion syntax:
/// `\1`/`\g<1>`/`\g<name>` become `${1}`/`${name}`, `\n`/`\t`/`\r`/`\\` become the
/// characters they denote, and literal `$` is escaped as `$$`.
pub(crate) fn replacement(template: &str) -> String {
    let mut out = String::with_capacity(template.len() + 8);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '$' => out.push_str("$$"),
            '\\' => match chars.next() {
                Some(d) if d.is_ascii_digit() => {
                    let mut group = String::from(d);
                    if let Some(&next) = chars.peek()
                        && next.is_ascii_digit()
                    {
                        group.push(next);
                        chars.next();
                    }
                    let _ = write!(out, "${{{group}}}");
                }
                Some('g') if chars.peek() == Some(&'<') => {
                    chars.next();
                    let name: String = chars.by_ref().take_while(|&c| c != '>').collect();
                    let _ = write!(out, "${{{name}}}");
                }
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                other => {
                    // Unknown escapes and a trailing backslash stay literal.
                    out.push('\\');
                    out.extend(other);
                }
            },
            other => out.push(other),
        }
    }
    out
}

/// Converts a Python pattern to `regex` syntax. The rule files are applied with
/// `re.MULTILINE`, so `(?m)` is added. `\Z` becomes `\z`, and escaped spaces (legal in
/// Python, rejected by `regex` outside verbose mode) become plain spaces.
pub(crate) fn pattern(python: &str) -> String {
    let mut out = String::with_capacity(python.len() + 4);
    out.push_str("(?m)");
    let mut chars = python.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(' ') => out.push(' '),
            Some('Z') => out.push_str(r"\z"),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_groups_and_escapes() {
        assert_eq!(replacement(r"\1\2)\n"), "${1}${2})\n");
        assert_eq!(
            replacement(r"\1 `edw_\3`.`\2_\4`"),
            "${1} `edw_${3}`.`${2}_${4}`"
        );
        assert_eq!(replacement(r"\g<name>$"), "${name}$$");
        assert_eq!(replacement(r"\12"), "${12}");
    }

    #[test]
    fn pattern_compat() {
        assert_eq!(pattern(r"\ +,CONSTRAINT"), "(?m) +,CONSTRAINT");
        assert_eq!(pattern(r"a\Z"), r"(?m)a\z");
        assert!(regex::Regex::new(&pattern(r"\ +,CONSTRAINT[^\n]+\n")).is_ok());
    }
}
