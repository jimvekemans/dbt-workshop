//! Statement classification: what does a statement do, and to which object?

use vdp_vaultspeed_domain::{AlterOp, ObjectRef, ObjectType, StatementKind};

use crate::lexer::{Token, TokenKind, tokenize};

/// Result of [`classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: StatementKind,
    pub target: Option<ObjectRef>,
}

impl Classification {
    const fn other() -> Self {
        Self {
            kind: StatementKind::Other,
            target: None,
        }
    }
}

/// Classifies a single statement by looking at its leading keywords.
///
/// Unknown shapes classify as [`StatementKind::Other`], which planners treat with the
/// most conservative policy (run once).
pub fn classify(sql: &str) -> Classification {
    let tokens = tokenize(sql);
    let mut cursor = Cursor::new(tokens.iter().filter(|t| !t.is_trivia()).copied().collect());
    let Some(first) = cursor.next_word() else {
        return Classification::other();
    };
    match first.as_str() {
        "CREATE" => classify_create(&mut cursor),
        "DROP" => classify_drop(&mut cursor),
        "ALTER" => classify_alter(&mut cursor),
        "TRUNCATE" => {
            cursor.eat_word("TABLE");
            dml(StatementKind::Truncate, &mut cursor)
        }
        "INSERT" => {
            cursor.eat_word("IGNORE");
            cursor.eat_word("INTO");
            dml(StatementKind::Insert, &mut cursor)
        }
        "REPLACE" => {
            cursor.eat_word("INTO");
            dml(StatementKind::Replace, &mut cursor)
        }
        "UPDATE" => dml(StatementKind::Update, &mut cursor),
        "DELETE" => {
            cursor.skip_until_word("FROM");
            dml(StatementKind::Delete, &mut cursor)
        }
        "MERGE" => {
            cursor.eat_word("INTO");
            dml(StatementKind::Merge, &mut cursor)
        }
        "CALL" => Classification {
            kind: StatementKind::Call,
            target: cursor.qualified_name().map(|(schema, name)| {
                ObjectRef::new(ObjectType::Procedure, schema.as_deref(), &name)
            }),
        },
        "SET" => simple(StatementKind::Set),
        "SELECT" | "WITH" => simple(StatementKind::Select),
        "USE" => simple(StatementKind::Use),
        _ => Classification::other(),
    }
}

/// Like [`classify`], but unqualified object names (and the target of a rename) are resolved to
/// `database`, the database in effect. In `SingleStore` the schema of an object is its database, so
/// `ALTER TABLE x ...` under `USE edw` must be the same object as `ALTER TABLE edw.x ...`. With no
/// database (unknown) the names stay unqualified.
pub fn classify_in(sql: &str, database: Option<&str>) -> Classification {
    let mut classification = classify(sql);
    let Some(database) = database.map(str::trim).filter(|d| !d.is_empty()) else {
        return classification;
    };
    let qualify = |object: &mut ObjectRef| {
        let lives_in_database = !matches!(
            object.object_type,
            ObjectType::Database | ObjectType::Index | ObjectType::Other
        );
        if lives_in_database && object.schema.is_none() {
            *object = ObjectRef::new(object.object_type, Some(database), &object.name);
        }
    };
    if let Some(target) = &mut classification.target {
        qualify(target);
    }
    if let StatementKind::Alter {
        op: AlterOp::Rename { to },
    } = &mut classification.kind
    {
        qualify(to);
    }
    classification
}

/// The database a `USE <database>` statement switches to; `None` for any other statement.
pub fn use_database(sql: &str) -> Option<String> {
    let tokens = tokenize(sql);
    let mut cursor = Cursor::new(tokens.iter().filter(|t| !t.is_trivia()).copied().collect());
    if !cursor.eat_word("USE") {
        return None;
    }
    cursor.identifier().map(|name| name.to_lowercase())
}

const fn simple(kind: StatementKind) -> Classification {
    Classification { kind, target: None }
}

fn dml(kind: StatementKind, cursor: &mut Cursor<'_>) -> Classification {
    Classification {
        kind,
        target: cursor.object(ObjectType::Table),
    }
}

fn object_type(word: &str) -> Option<ObjectType> {
    match word {
        "TABLE" => Some(ObjectType::Table),
        "VIEW" => Some(ObjectType::View),
        "PROCEDURE" => Some(ObjectType::Procedure),
        "FUNCTION" => Some(ObjectType::Function),
        "DATABASE" | "SCHEMA" => Some(ObjectType::Database),
        "INDEX" => Some(ObjectType::Index),
        "TRIGGER" | "EVENT" | "PIPELINE" | "LINK" | "USER" | "ROLE" | "GROUP" => {
            Some(ObjectType::Other)
        }
        _ => None,
    }
}

fn classify_create(cursor: &mut Cursor<'_>) -> Classification {
    let or_replace = cursor.eat_words(&["OR", "REPLACE"]);
    // Skip modifiers such as TEMPORARY, ROWSTORE, REFERENCE, ALGORITHM=x, DEFINER=`u`@`h`,
    // SQL SECURITY DEFINER, AGGREGATE until the object keyword.
    let Some(object) = cursor.find_word_map(object_type) else {
        return Classification::other();
    };
    let if_not_exists = cursor.eat_words(&["IF", "NOT", "EXISTS"]);
    Classification {
        kind: StatementKind::Create {
            object,
            or_replace,
            if_not_exists,
        },
        target: cursor.object(object),
    }
}

fn classify_drop(cursor: &mut Cursor<'_>) -> Classification {
    cursor.eat_word("TEMPORARY");
    let Some(object) = cursor.next_word().as_deref().and_then(object_type) else {
        return Classification::other();
    };
    let if_exists = cursor.eat_words(&["IF", "EXISTS"]);
    Classification {
        kind: StatementKind::Drop { object, if_exists },
        target: cursor.object(object),
    }
}

fn classify_alter(cursor: &mut Cursor<'_>) -> Classification {
    let Some(object) = cursor.next_word().as_deref().and_then(object_type) else {
        return Classification::other();
    };
    let Some(target) = cursor.object(object) else {
        return Classification::other();
    };
    let op = if object == ObjectType::Table {
        alter_table_op(cursor, &target)
    } else {
        AlterOp::Other
    };
    Classification {
        kind: StatementKind::Alter { op },
        target: Some(target),
    }
}

/// `ALTER TABLE ADD|DROP <word>` forms that do not touch a column.
const NON_COLUMN: [&str; 9] = [
    "INDEX",
    "KEY",
    "PRIMARY",
    "UNIQUE",
    "CONSTRAINT",
    "FULLTEXT",
    "SHARD",
    "SORT",
    "PARTITION",
];

fn alter_table_op(cursor: &mut Cursor<'_>, table: &ObjectRef) -> AlterOp {
    if cursor.has_top_level_comma() {
        return AlterOp::Multiple;
    }
    let Some(word) = cursor.next_word() else {
        return AlterOp::Other;
    };
    match word.as_str() {
        "ADD" => {
            if cursor
                .peek_word()
                .is_some_and(|w| NON_COLUMN.contains(&w.as_str()))
            {
                return AlterOp::Other;
            }
            cursor.eat_word("COLUMN");
            match cursor.identifier() {
                Some(column) => AlterOp::AddColumn {
                    column: column.to_lowercase(),
                    data_type: cursor.data_type(),
                },
                None => AlterOp::Other,
            }
        }
        "MODIFY" | "CHANGE" | "DROP" => {
            if cursor
                .peek_word()
                .is_some_and(|w| NON_COLUMN.contains(&w.as_str()))
            {
                return AlterOp::Other;
            }
            cursor.eat_word("COLUMN");
            let Some(column) = cursor.identifier().map(|c| c.to_lowercase()) else {
                return AlterOp::Other;
            };
            match word.as_str() {
                "MODIFY" => AlterOp::ModifyColumn {
                    column,
                    data_type: cursor.data_type(),
                },
                "CHANGE" => AlterOp::ChangeColumn { column },
                _ => AlterOp::DropColumn { column },
            }
        }
        "RENAME" => {
            if !(cursor.eat_word("TO") || cursor.eat_word("AS"))
                && cursor.peek_word().as_deref() == Some("COLUMN")
            {
                return AlterOp::Other;
            }
            match cursor.qualified_name() {
                Some((schema, name)) => AlterOp::Rename {
                    to: ObjectRef::new(
                        ObjectType::Table,
                        schema.as_deref().or(table.schema.as_deref()),
                        &name,
                    ),
                },
                None => AlterOp::Other,
            }
        }
        "COMMENT" => AlterOp::Comment,
        _ => AlterOp::Other,
    }
}

/// Forward-only cursor over significant tokens.
struct Cursor<'a> {
    tokens: Vec<Token<'a>>,
    pos: usize,
}

impl<'a> Cursor<'a> {
    const fn new(tokens: Vec<Token<'a>>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.pos)
    }

    fn peek_word(&self) -> Option<String> {
        self.peek()
            .filter(|t| t.kind == TokenKind::Word)
            .map(|t| t.text.to_ascii_uppercase())
    }

    fn next_word(&mut self) -> Option<String> {
        let word = self.peek_word()?;
        self.pos += 1;
        Some(word)
    }

    fn eat_word(&mut self, keyword: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_word(keyword)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Consumes the keywords only if all of them follow in sequence.
    fn eat_words(&mut self, keywords: &[&str]) -> bool {
        let matches = keywords
            .iter()
            .enumerate()
            .all(|(i, k)| self.tokens.get(self.pos + i).is_some_and(|t| t.is_word(k)));
        if matches {
            self.pos += keywords.len();
        }
        matches
    }

    fn skip_until_word(&mut self, keyword: &str) {
        while let Some(token) = self.peek().copied() {
            self.pos += 1;
            if token.is_word(keyword) {
                return;
            }
        }
    }

    /// Advances to the first word accepted by `map`, consuming it.
    fn find_word_map<T>(&mut self, map: impl Fn(&str) -> Option<T>) -> Option<T> {
        while let Some(token) = self.peek().copied() {
            self.pos += 1;
            if token.kind == TokenKind::Word
                && let Some(found) = map(&token.text.to_ascii_uppercase())
            {
                return Some(found);
            }
            if token.kind == TokenKind::Punct && token.text == "(" {
                return None;
            }
        }
        None
    }

    fn identifier(&mut self) -> Option<String> {
        let id = self.peek()?.identifier()?;
        self.pos += 1;
        Some(id)
    }

    /// `name` or `schema.name`.
    fn qualified_name(&mut self) -> Option<(Option<String>, String)> {
        let first = self.identifier()?;
        if self
            .peek()
            .is_some_and(|t| t.kind == TokenKind::Punct && t.text == ".")
        {
            self.pos += 1;
            let second = self.identifier()?;
            Some((Some(first), second))
        } else {
            Some((None, first))
        }
    }

    fn object(&mut self, object_type: ObjectType) -> Option<ObjectRef> {
        self.qualified_name()
            .map(|(schema, name)| ObjectRef::new(object_type, schema.as_deref(), &name))
    }

    fn has_top_level_comma(&self) -> bool {
        let mut depth = 0_i32;
        for token in &self.tokens[self.pos..] {
            match (token.kind, token.text) {
                (TokenKind::Punct, "(") => depth += 1,
                (TokenKind::Punct, ")") => depth -= 1,
                (TokenKind::Punct, ",") if depth == 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// Column data type up to the first column attribute, e.g. `varchar(6)` or
    /// `decimal(10,2) unsigned`.
    fn data_type(&mut self) -> String {
        const ATTRIBUTES: [&str; 16] = [
            "NOT",
            "NULL",
            "DEFAULT",
            "COMMENT",
            "AFTER",
            "FIRST",
            "AUTO_INCREMENT",
            "PRIMARY",
            "UNIQUE",
            "KEY",
            "COLLATE",
            "CHARACTER",
            "CHARSET",
            "ON",
            "SPARSE",
            "SERIES",
        ];
        let mut out = String::new();
        let mut previous_was_word = false;
        while let Some(token) = self.peek().copied() {
            let is_word = token.kind == TokenKind::Word;
            if token.kind == TokenKind::Semicolon
                || (is_word && ATTRIBUTES.contains(&token.text.to_ascii_uppercase().as_str()))
            {
                break;
            }
            if is_word && previous_was_word {
                out.push(' ');
            }
            out.push_str(&token.text.to_lowercase());
            previous_was_word = is_word;
            self.pos += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(c: &Classification) -> String {
        c.target
            .as_ref()
            .map(ObjectRef::qualified_name)
            .unwrap_or_default()
    }

    #[test]
    fn create_variants() {
        let c = classify("CREATE OR REPLACE PROCEDURE `edw_proc`.`p`() AS BEGIN END");
        assert_eq!(
            c.kind,
            StatementKind::Create {
                object: ObjectType::Procedure,
                or_replace: true,
                if_not_exists: false
            }
        );
        assert_eq!(target(&c), "edw_proc.p");

        let c = classify("/* hdr */ CREATE ROWSTORE REFERENCE TABLE IF NOT EXISTS s.t (a INT)");
        assert_eq!(
            c.kind,
            StatementKind::Create {
                object: ObjectType::Table,
                or_replace: false,
                if_not_exists: true
            }
        );
        assert_eq!(target(&c), "s.t");

        let c = classify("CREATE  VIEW `competent_dfv`.`vw_x`  AS SELECT 1");
        assert!(matches!(
            c.kind,
            StatementKind::Create {
                object: ObjectType::View,
                ..
            }
        ));
        assert_eq!(c.target.unwrap().object_type, ObjectType::View);
    }

    #[test]
    fn create_definer_view() {
        let c = classify(
            "CREATE ALGORITHM=MERGE DEFINER=`root`@`%` SQL SECURITY DEFINER VIEW v.w AS SELECT 1",
        );
        assert!(matches!(
            c.kind,
            StatementKind::Create {
                object: ObjectType::View,
                ..
            }
        ));
        assert_eq!(target(&c), "v.w");
    }

    #[test]
    fn drops() {
        let c = classify("DROP VIEW IF EXISTS `edw_dv_bdv`.`ref_ami_taalcodes`");
        assert_eq!(
            c.kind,
            StatementKind::Drop {
                object: ObjectType::View,
                if_exists: true
            }
        );
        assert_eq!(target(&c), "edw_dv_bdv.ref_ami_taalcodes");
    }

    #[test]
    fn alter_add_column_extracts_column_and_type() {
        let c = classify(
            "ALTER TABLE `edw_dv_rdv`.`sat_smp_sluitendmaatpak`\n\tADD COLUMN `gemigreerd_naar_axon` DECIMAL(2);",
        );
        assert_eq!(
            c.kind,
            StatementKind::Alter {
                op: AlterOp::AddColumn {
                    column: "gemigreerd_naar_axon".into(),
                    data_type: "decimal(2)".into()
                }
            }
        );
        let c = classify("ALTER TABLE t ADD c VARCHAR(10) NOT NULL DEFAULT 'x'");
        assert!(matches!(
            c.kind,
            StatementKind::Alter { op: AlterOp::AddColumn { ref data_type, .. } } if data_type == "varchar(10)"
        ));
    }

    #[test]
    fn alter_rename_inherits_schema() {
        let c = classify("ALTER TABLE `edw_dv_rdv`.`sat_a`\n\tRENAME TO `sat_a_v382`");
        let StatementKind::Alter {
            op: AlterOp::Rename { to },
        } = c.kind
        else {
            panic!("expected rename, got {:?}", c.kind);
        };
        assert_eq!(to.qualified_name(), "edw_dv_rdv.sat_a_v382");
    }

    #[test]
    fn unqualified_names_resolve_to_the_database_in_effect() {
        let c = classify_in("ALTER TABLE `X` RENAME TO `x_old`", Some("EDW"));
        assert_eq!(target(&c), "edw.x");
        let StatementKind::Alter {
            op: AlterOp::Rename { to },
        } = c.kind
        else {
            panic!("expected rename");
        };
        assert_eq!(to.qualified_name(), "edw.x_old");

        // Qualified names and databases are left alone; so is an unknown database.
        let c = classify_in("CREATE TABLE other.t (a INT)", Some("edw"));
        assert_eq!(target(&c), "other.t");
        let c = classify_in("CREATE DATABASE d", Some("edw"));
        assert_eq!(c.target.unwrap().qualified_name(), "d");
        let c = classify_in("INSERT INTO t SELECT 1", Some(""));
        assert_eq!(c.target.unwrap().qualified_name(), "t");
    }

    #[test]
    fn use_statements_name_their_database() {
        assert_eq!(use_database("USE `EDW_Meta`").as_deref(), Some("edw_meta"));
        assert_eq!(use_database("use x;").as_deref(), Some("x"));
        assert_eq!(use_database("SELECT 1"), None);
    }

    #[test]
    fn alter_comment_multiple_and_index() {
        assert_eq!(
            classify("ALTER TABLE t COMMENT 'DV_NAME: edw, x'").kind,
            StatementKind::Alter {
                op: AlterOp::Comment
            }
        );
        assert_eq!(
            classify("ALTER TABLE t ADD COLUMN a INT, ADD COLUMN b INT").kind,
            StatementKind::Alter {
                op: AlterOp::Multiple
            }
        );
        assert_eq!(
            classify("ALTER TABLE t ADD INDEX i (a)").kind,
            StatementKind::Alter { op: AlterOp::Other }
        );
        assert_eq!(
            classify("ALTER TABLE t MODIFY COLUMN `a` VARCHAR(90) NULL").kind,
            StatementKind::Alter {
                op: AlterOp::ModifyColumn {
                    column: "a".into(),
                    data_type: "varchar(90)".into()
                }
            }
        );
        assert_eq!(
            classify("ALTER TABLE t DROP COLUMN `a`").kind,
            StatementKind::Alter {
                op: AlterOp::DropColumn { column: "a".into() }
            }
        );
    }

    #[test]
    fn dml_targets() {
        assert_eq!(target(&classify("INSERT INTO s.t SELECT 1")), "s.t");
        assert_eq!(target(&classify("DELETE t FROM s.t t WHERE 1")), "s.t");
        assert_eq!(target(&classify("CALL `edw_proc`.`x`()")), "edw_proc.x");
        assert_eq!(classify("SET SQL_MODE = X").kind, StatementKind::Set);
        assert_eq!(classify("BEGIN").kind, StatementKind::Other);
    }
}
