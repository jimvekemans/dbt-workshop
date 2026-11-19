//! SQL scripts as text: where statements start and end.
//!
//! A building block with no dependencies on the other `vdp-*` crates. It is used by the database session (a script runs
//! statement by statement, so its last result set is the one shown) and by VaultSpeed deployments.
//!
//! # Responsibilities
//!
//! - [`lexer`]: knows where strings, quoted identifiers and comments start and end ([`tokenize`]).
//! - [`split`]: cuts a script into statements, keeping routine bodies (`BEGIN … END`, `DELIMITER`) whole
//!   ([`RoutineAwareSplitter`]), or keeps a file whole ([`WholeFileSplitter`]), behind the [`StatementSplitter`] trait.
//! - Tell whether a text holds any SQL at all ([`has_sql`]).
//!
//! # Example
//!
//! ```
//! use vdp_sql_script::{RoutineAwareSplitter, StatementSplitter, has_sql};
//!
//! let script = "SELECT ';' AS semicolon; -- done\nSELECT 2;";
//! let statements = RoutineAwareSplitter.split(script);
//! assert_eq!(statements.len(), 2); // the `;` inside the string does not split
//! assert!(has_sql(statements[1]));
//! assert!(!has_sql("-- only a comment"));
//! ```

pub mod lexer;
pub mod split;

pub use lexer::{Token, TokenKind, tokenize};
pub use split::{RoutineAwareSplitter, StatementSplitter, WholeFileSplitter, has_sql};
