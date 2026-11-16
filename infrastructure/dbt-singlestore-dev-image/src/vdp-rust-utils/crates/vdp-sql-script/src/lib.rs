//! SQL scripts as text: where statements start and end.
//!
//! [`lexer`] knows where strings, quoted identifiers and comments start and end; [`split`] cuts a script into
//! statements, keeping routine bodies (`BEGIN … END`, `DELIMITER`) whole. Used by the database session (a script
//! runs statement by statement, so its last result set is the one shown) and by VaultSpeed deployments.

pub mod lexer;
pub mod split;

pub use lexer::{Token, TokenKind, tokenize};
pub use split::{RoutineAwareSplitter, StatementSplitter, WholeFileSplitter, has_sql};
