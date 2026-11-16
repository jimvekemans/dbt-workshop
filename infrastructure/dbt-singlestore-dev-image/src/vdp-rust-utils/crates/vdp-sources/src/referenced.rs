//! Which source tables the project uses.

use std::collections::BTreeSet;

use vdp_dbt::artifacts::Manifest;

/// `(source_name, table)` pairs used by nodes (`depends_on`) or named in macros (`source('a', 'b')`), as
/// `determine_referenced_sources` did.
pub fn referenced_sources(manifest: &Manifest) -> BTreeSet<(String, String)> {
    let mut used = BTreeSet::new();
    for node in manifest.nodes.values() {
        for dependency in &node.depends_on.nodes {
            if let Some(source) = manifest.sources.get(dependency) {
                used.insert((source.source_name.clone(), source.name.clone()));
            }
        }
    }
    // Sources used in macros do not appear in any depends_on.
    for macro_def in manifest.macros.values() {
        for (source, table) in source_calls(&macro_def.macro_sql) {
            let id = format!("source.{}.{source}.{table}", macro_def.package_name);
            if manifest.sources.contains_key(&id) {
                used.insert((source, table));
            }
        }
    }
    used
}

/// `source('a', 'b')` / `source("a","b")` calls with word-character arguments.
fn source_calls(sql: &str) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    let mut rest = sql;
    while let Some(start) = rest.find("source") {
        rest = &rest[start + "source".len()..];
        if let Some((args, tail)) = parse_call(rest) {
            calls.push(args);
            rest = tail;
        }
    }
    calls
}

fn parse_call(text: &str) -> Option<((String, String), &str)> {
    let text = text.trim_start().strip_prefix('(')?;
    let (first, text) = quoted_word(text.trim_start())?;
    let text = text.trim_start().strip_prefix(',')?;
    let (second, text) = quoted_word(text.trim_start())?;
    let text = text.trim_start().strip_prefix(')')?;
    Some(((first, second), text))
}

fn quoted_word(text: &str) -> Option<(String, &str)> {
    let quote = text.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let body = &text[1..];
    let end = body.find(quote)?;
    let word = &body[..end];
    (!word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .then(|| (word.to_owned(), &body[end + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_source_calls_like_the_python_regex() {
        let sql = "select * from {{ source('raw', 'a') }} join {{ source( \"raw\" ,\"b\" ) }} {{ source(var('x'), 'c') }}";
        assert_eq!(
            source_calls(sql),
            [
                ("raw".to_owned(), "a".to_owned()),
                ("raw".to_owned(), "b".to_owned())
            ]
        );
    }
}
