//! Normalisation and fingerprinting of SQL text.

use vdp_vaultspeed_domain::Fingerprint;

use crate::lexer::{TokenKind, tokenize};

/// Canonical form of a statement used for identity.
///
/// Comments and whitespace are dropped, unquoted words and quoted identifiers are
/// lower-cased (and unquoted), literals are kept verbatim and trailing semicolons are
/// removed. Two statements that differ only in layout, comments (`VaultSpeed` headers
/// carry generation dates) or identifier quoting normalise to the same text.
///
/// Exception: *hint comments*. VaultSpeed emits clauses it cannot express in its target
/// dialect as line comments (`-- , SHARD KEY (`hub_hkey`)`) and the DDL rewrite rules turn
/// them into SQL. Those comments change what executes, so they are part of the
/// identity (in normalised form); a changed shard key is a changed statement.
pub fn normalize(sql: &str) -> String {
    let tokens = tokenize(sql);
    let significant: Vec<_> = tokens
        .iter()
        .filter(|t| !t.is_trivia() || hint_comment(t).is_some())
        .collect();
    let end = significant
        .iter()
        .rposition(|t| t.kind != TokenKind::Semicolon)
        .map_or(0, |last| last + 1);

    let mut out = String::with_capacity(sql.len());
    for token in &significant[..end] {
        if !out.is_empty() {
            out.push(' ');
        }
        if let Some(hint) = hint_comment(token) {
            out.push_str("--hint ");
            out.push_str(&normalize(hint));
            continue;
        }
        match token.kind {
            TokenKind::Word => out.push_str(&token.text.to_lowercase()),
            TokenKind::QuotedIdent => {
                out.push_str(&token.identifier().unwrap_or_default().to_lowercase());
            }
            _ => out.push_str(token.text),
        }
    }
    out
}

/// Body of a `--` comment that carries a table clause (`SHARD KEY`, `SORT KEY`, `KEY`,
/// `UNIQUE`, `PRIMARY KEY`, `CONSTRAINT`, `INDEX`), e.g. ` , SHARD KEY (a)`.
fn hint_comment<'a>(token: &crate::Token<'a>) -> Option<&'a str> {
    const HINTS: [&str; 7] = [
        "SHARD KEY",
        "SORT KEY",
        "PRIMARY KEY",
        "UNIQUE",
        "CONSTRAINT",
        "INDEX",
        "KEY",
    ];
    let body = token
        .text
        .strip_prefix("--")
        .filter(|_| token.kind == TokenKind::LineComment)?;
    let clause = body.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    let words = clause
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    HINTS
        .iter()
        .any(|hint| {
            words.strip_prefix(hint).is_some_and(|rest| {
                !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
            })
        })
        .then_some(body)
}

/// Fingerprint of the normalised statement.
pub fn fingerprint(sql: &str) -> Fingerprint {
    Fingerprint::of(normalize(sql))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_comments_and_quoting_do_not_matter() {
        let a = "/* generated 2025/01/14 */\nALTER TABLE `edw_dv_rdv`.`sat_x`\n\tADD COLUMN `y` VARCHAR(6);";
        let b = "-- generated 2025/02/01\nalter table edw_dv_rdv.sat_x add column y varchar ( 6 )";
        assert_eq!(fingerprint(a), fingerprint(b));
    }

    #[test]
    fn hint_comments_are_part_of_the_identity() {
        let table = |hint: &str| fingerprint(&format!("CREATE TABLE s.t (a INT, b INT)\n{hint}\n"));
        assert_ne!(table("-- , SHARD KEY (`a`)"), table("-- , SHARD KEY (`b`)"));
        assert_eq!(
            table("-- , SHARD KEY (`a`)"),
            table("--   ,  shard key ( a )")
        );
        assert_eq!(
            table("-- START"),
            table("-- keyword soup"),
            "ordinary comments still ignored"
        );
        assert_ne!(table(""), table("-- , SORT KEY (`a`)"));
    }

    #[test]
    fn literals_matter() {
        assert_ne!(
            fingerprint("ALTER TABLE t COMMENT 'Release 1'"),
            fingerprint("ALTER TABLE t COMMENT 'Release 2'")
        );
    }

    #[test]
    fn identifiers_matter() {
        assert_ne!(
            fingerprint("ALTER TABLE t ADD COLUMN a INT"),
            fingerprint("ALTER TABLE t ADD COLUMN b INT")
        );
    }

    #[test]
    fn normal_form_is_readable() {
        assert_eq!(
            normalize("DROP TABLE  IF EXISTS `S`.`T` ; "),
            "drop table if exists s . t"
        );
    }
}
