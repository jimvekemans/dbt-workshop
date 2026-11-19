//! A small MySQL/SingleStore lexer.
//!
//! It is deliberately not a parser: it only needs to know where strings, quoted
//! identifiers and comments start and end, so that `;`, keywords and whitespace can be
//! interpreted safely. `VaultSpeed` procedure bodies are not parseable by general-purpose
//! SQL parsers, but they lex fine.

/// Lexical category of a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Keyword or unquoted identifier (also `@var` and `$x`).
    Word,
    /// `` `identifier` ``
    QuotedIdent,
    /// `'text'` or `"text"`
    StringLit,
    Number,
    Whitespace,
    /// `-- ...` or `# ...` up to (not including) the newline.
    LineComment,
    /// `/* ... */`
    BlockComment,
    Semicolon,
    /// Any other single character.
    Punct,
}

/// A token borrowing its text from the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
    /// Byte offset of the token in the input.
    pub start: usize,
}

impl Token<'_> {
    /// Whitespace and comments carry no meaning for execution decisions.
    pub const fn is_trivia(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Whitespace | TokenKind::LineComment | TokenKind::BlockComment
        )
    }

    /// Whether the token is a line or block comment.
    pub const fn is_comment(&self) -> bool {
        matches!(self.kind, TokenKind::LineComment | TokenKind::BlockComment)
    }

    /// Case-insensitive keyword comparison for [`TokenKind::Word`] tokens.
    pub fn is_word(&self, keyword: &str) -> bool {
        self.kind == TokenKind::Word && self.text.eq_ignore_ascii_case(keyword)
    }

    /// The byte offset just past the token.
    pub const fn end(&self) -> usize {
        self.start + self.text.len()
    }

    /// Identifier text without quoting, for words and quoted identifiers.
    pub fn identifier(&self) -> Option<String> {
        match self.kind {
            TokenKind::Word => Some(self.text.to_owned()),
            TokenKind::QuotedIdent => {
                let inner = &self.text[1..self.text.len().saturating_sub(1).max(1)];
                Some(inner.replace("``", "`"))
            }
            _ => None,
        }
    }
}

/// Splits `input` into tokens; concatenating all token texts yields `input` again.
pub fn tokenize(input: &str) -> Vec<Token<'_>> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < bytes.len() {
        let start = pos;
        let c = input[pos..]
            .chars()
            .next()
            .expect("pos is on a char boundary");
        let kind = match c {
            c if c.is_whitespace() => {
                pos = scan_while(input, pos, char::is_whitespace);
                TokenKind::Whitespace
            }
            '-' if bytes.get(pos + 1) == Some(&b'-') => {
                pos = scan_line(input, pos);
                TokenKind::LineComment
            }
            '#' => {
                pos = scan_line(input, pos);
                TokenKind::LineComment
            }
            '/' if bytes.get(pos + 1) == Some(&b'*') => {
                pos = input[pos + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| pos + 2 + end + 2);
                TokenKind::BlockComment
            }
            '\'' | '"' => {
                pos = scan_quoted(bytes, pos, bytes[pos], true);
                TokenKind::StringLit
            }
            '`' => {
                pos = scan_quoted(bytes, pos, b'`', false);
                TokenKind::QuotedIdent
            }
            ';' => {
                pos += 1;
                TokenKind::Semicolon
            }
            c if c.is_ascii_digit() => {
                pos = scan_while(input, pos, |c| c.is_alphanumeric() || c == '.' || c == '_');
                TokenKind::Number
            }
            c if is_word_start(c) => {
                pos = scan_while(input, pos + c.len_utf8(), is_word_char);
                TokenKind::Word
            }
            c => {
                pos += c.len_utf8();
                TokenKind::Punct
            }
        };
        tokens.push(Token {
            kind,
            text: &input[start..pos],
            start,
        });
    }
    tokens
}

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '@' | '$')
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '@')
}

fn scan_while(input: &str, from: usize, predicate: impl Fn(char) -> bool) -> usize {
    input[from..]
        .char_indices()
        .find(|(_, c)| !predicate(*c))
        .map_or(input.len(), |(offset, _)| from + offset)
}

fn scan_line(input: &str, from: usize) -> usize {
    input[from..]
        .find('\n')
        .map_or(input.len(), |end| from + end)
}

/// Scans a quoted run starting at `from` (which holds the quote). A doubled quote is an
/// escaped quote; with `backslash_escapes`, `\x` escapes any character. Unterminated
/// quotes run to the end of the input.
fn scan_quoted(bytes: &[u8], from: usize, quote: u8, backslash_escapes: bool) -> usize {
    let mut pos = from + 1;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' if backslash_escapes => pos += 2,
            b if b == quote => {
                if bytes.get(pos + 1) == Some(&quote) {
                    pos += 2;
                } else {
                    return pos + 1;
                }
            }
            _ => pos += 1,
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<(TokenKind, &str)> {
        tokenize(input)
            .into_iter()
            .filter(|t| t.kind != TokenKind::Whitespace)
            .map(|t| (t.kind, t.text))
            .collect()
    }

    #[test]
    fn round_trips_input() {
        let input = "SELECT 'a;b', `c``d` -- x;\n/* y; */ FROM t; # z\nüñï";
        let rebuilt: String = tokenize(input).iter().map(|t| t.text).collect();
        assert_eq!(rebuilt, input);
    }

    #[test]
    fn semicolons_inside_strings_and_comments_are_not_terminators() {
        let semis = tokenize("SELECT 'a;b' /* ; */ -- ;\n;")
            .iter()
            .filter(|t| t.kind == TokenKind::Semicolon)
            .count();
        assert_eq!(semis, 1);
    }

    #[test]
    fn handles_escapes() {
        assert_eq!(
            kinds(r"'it''s' 'a\'b' `x``y`"),
            vec![
                (TokenKind::StringLit, "'it''s'"),
                (TokenKind::StringLit, r"'a\'b'"),
                (TokenKind::QuotedIdent, "`x``y`"),
            ]
        );
        assert_eq!(tokenize("`x``y`")[0].identifier().unwrap(), "x`y");
    }

    #[test]
    fn unterminated_constructs_run_to_end() {
        assert_eq!(kinds("/* open"), vec![(TokenKind::BlockComment, "/* open")]);
        assert_eq!(kinds("'open"), vec![(TokenKind::StringLit, "'open")]);
    }

    #[test]
    fn minus_is_punct_unless_doubled() {
        assert_eq!(
            kinds("a-1 --c"),
            vec![
                (TokenKind::Word, "a"),
                (TokenKind::Punct, "-"),
                (TokenKind::Number, "1"),
                (TokenKind::LineComment, "--c"),
            ]
        );
    }
}
