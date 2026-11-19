//! Splitting script text into individually executable statements.

use std::fmt::Debug;

use crate::lexer::{Token, TokenKind, tokenize};

/// Turns the text of one file into statement texts, in order.
///
/// Returned fragments keep their leading comments (`VaultSpeed` headers, `-- START`
/// markers) so that rewrite rules written against the Python implementation still
/// see them; fragments without any SQL are dropped.
pub trait StatementSplitter: Send + Sync + Debug {
    /// The statements of `script`, as slices of it, without their terminators.
    fn split<'a>(&self, script: &'a str) -> Vec<&'a str>;
}

/// The whole file is a single statement.
#[derive(Debug, Clone, Copy, Default)]
pub struct WholeFileSplitter;

impl StatementSplitter for WholeFileSplitter {
    fn split<'a>(&self, script: &'a str) -> Vec<&'a str> {
        let trimmed = script.trim();
        if has_sql(trimmed) {
            vec![trimmed]
        } else {
            Vec::new()
        }
    }
}

/// Splits on top-level `;`, treating stored routine bodies as one statement.
///
/// Inside `CREATE ... PROCEDURE|FUNCTION|TRIGGER|EVENT`, `BEGIN`/`CASE` open a block and
/// `END` closes one (`END IF`, `END LOOP`, `END WHILE`, `END REPEAT`, `END FOR` close
/// constructs that never opened a counted block). The routine ends at the first `;`
/// after its outermost block has closed, so `SingleStore`'s
/// `AS DECLARE x INT = 0; BEGIN ... END;` form is kept intact.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoutineAwareSplitter;

impl StatementSplitter for RoutineAwareSplitter {
    fn split<'a>(&self, script: &'a str) -> Vec<&'a str> {
        let tokens = tokenize(script);
        let mut fragments = Vec::new();
        let mut state = StatementState::default();
        let mut fragment_start = 0;

        for (index, token) in tokens.iter().enumerate() {
            if token.is_trivia() {
                continue;
            }
            if token.kind == TokenKind::Semicolon {
                if state.may_terminate() {
                    push_fragment(&mut fragments, &script[fragment_start..token.start]);
                    fragment_start = token.end();
                    state = StatementState::default();
                }
                continue;
            }
            state.observe(token, next_word(&tokens[index + 1..]));
        }
        push_fragment(&mut fragments, &script[fragment_start..]);
        fragments
    }
}

/// Tracks whether a `;` ends the current statement.
#[derive(Debug, Default)]
struct StatementState {
    /// Leading words of the statement, upper-cased, until routine detection is decided.
    head: Vec<String>,
    head_decided: bool,
    in_routine: bool,
    depth: u32,
    body_closed: bool,
}

impl StatementState {
    const HEAD_LIMIT: usize = 12;
    const ROUTINE_OBJECTS: [&'static str; 4] = ["PROCEDURE", "FUNCTION", "TRIGGER", "EVENT"];
    const NON_BLOCK_ENDS: [&'static str; 5] = ["IF", "LOOP", "WHILE", "REPEAT", "FOR"];

    fn may_terminate(&self) -> bool {
        !self.in_routine || self.body_closed
    }

    fn observe(&mut self, token: &Token<'_>, next_word: Option<&str>) {
        if !self.head_decided {
            self.observe_head(token);
        }
        if !self.in_routine || token.kind != TokenKind::Word {
            return;
        }
        if token.is_word("BEGIN") || token.is_word("CASE") {
            self.depth += 1;
        } else if token.is_word("END") {
            let closes_block = !next_word.is_some_and(|w| {
                Self::NON_BLOCK_ENDS
                    .iter()
                    .any(|end| end.eq_ignore_ascii_case(w))
            });
            if closes_block && self.depth > 0 {
                self.depth -= 1;
                if self.depth == 0 {
                    self.body_closed = true;
                }
            }
        }
    }

    fn observe_head(&mut self, token: &Token<'_>) {
        if token.kind == TokenKind::Word {
            self.head.push(token.text.to_ascii_uppercase());
        }
        let starts_with_create = self.head.first().is_some_and(|w| w == "CREATE");
        if !starts_with_create && !self.head.is_empty() {
            self.head_decided = true;
            return;
        }
        if self
            .head
            .iter()
            .any(|w| Self::ROUTINE_OBJECTS.contains(&w.as_str()))
        {
            self.in_routine = true;
            self.head_decided = true;
        } else if self
            .head
            .iter()
            .any(|w| matches!(w.as_str(), "TABLE" | "VIEW" | "DATABASE" | "INDEX"))
            || self.head.len() >= Self::HEAD_LIMIT
        {
            self.head_decided = true;
        }
    }
}

fn next_word<'a>(rest: &[Token<'a>]) -> Option<&'a str> {
    rest.iter()
        .find(|t| !t.is_trivia())
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.text)
}

fn push_fragment<'a>(fragments: &mut Vec<&'a str>, fragment: &'a str) {
    let trimmed = fragment.trim();
    if has_sql(trimmed) {
        fragments.push(trimmed);
    }
}

/// Whether the text contains anything besides whitespace and comments.
pub fn has_sql(text: &str) -> bool {
    tokenize(text)
        .iter()
        .any(|t| !t.is_trivia() && t.kind != TokenKind::Semicolon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(script: &str) -> Vec<&str> {
        RoutineAwareSplitter.split(script)
    }

    #[test]
    fn splits_plain_ddl_and_drops_comment_only_fragments() {
        let script = "/* header */\n-- START\nDROP TABLE IF EXISTS `a`.`b`;\n\n-- END\n\
                      CREATE TABLE `a`.`b` (x INT);\nALTER TABLE `a`.`b` COMMENT 'v;1';\n-- END\n";
        let parts = split(script);
        assert_eq!(parts.len(), 3);
        assert!(parts[0].starts_with("/* header */"));
        assert!(parts[0].ends_with("`a`.`b`"));
        assert!(parts[1].contains("CREATE TABLE"));
        assert_eq!(parts[2], "ALTER TABLE `a`.`b` COMMENT 'v;1'");
    }

    #[test]
    fn keeps_vaultspeed_procedures_whole() {
        let script = "CREATE OR REPLACE PROCEDURE `edw_proc`.`p`() \nAS\nBEGIN \n\
            SET SQL_MODE = PIPES_AS_CONCAT;\nBEGIN -- ref\n DELETE FROM t WHERE x = 1;\nEND;\n\
            INSERT INTO t SELECT CASE WHEN a THEN 1 ELSE 2 END FROM s;\nEND;\n\n\
            CREATE OR REPLACE PROCEDURE `edw_proc`.`q`() AS BEGIN SELECT 1; END;";
        let parts = split(script);
        assert_eq!(parts.len(), 2, "{parts:#?}");
        assert!(parts[0].ends_with("END"));
        assert!(parts[1].contains("`q`"));
    }

    #[test]
    fn handles_declare_sections_and_control_flow() {
        let script = "CREATE PROCEDURE p() AS DECLARE x INT = 0; BEGIN \
            IF x = 0 THEN SET x = 1; END IF; WHILE x < 3 LOOP SET x = x + 1; END LOOP; END; SELECT 2";
        let parts = split(script);
        assert_eq!(parts.len(), 2, "{parts:#?}");
        assert_eq!(parts[1], "SELECT 2");
    }

    #[test]
    fn views_with_case_expressions_still_split() {
        let script =
            "DROP VIEW IF EXISTS v; CREATE VIEW v AS SELECT CASE WHEN 1 THEN 2 END AS c; SELECT 1;";
        assert_eq!(split(script).len(), 3);
    }

    #[test]
    fn whole_file_splitter_ignores_empty_files() {
        assert_eq!(
            WholeFileSplitter.split(" -- nothing\n/* here */ "),
            [] as [&str; 0]
        );
        assert_eq!(WholeFileSplitter.split(" SELECT 1; ").len(), 1);
    }
}
