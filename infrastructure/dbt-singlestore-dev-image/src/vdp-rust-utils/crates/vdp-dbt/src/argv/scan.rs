//! Reads an argv the way dbt's clap parser does, without changing it.

use std::ffi::OsString;

use crate::flags::{Arity, Grammar, Scope};

/// Where a flag's value sits in argv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSlot {
    /// The whole token is the value (`--vars` `{a: 1}`).
    Token(usize),
    /// The value is the tail of a token from this byte offset (`--vars={a: 1}`, `-sx`, `-s=x`).
    Inline { token: usize, offset: usize },
}

impl ValueSlot {
    pub const fn token(self) -> usize {
        match self {
            Self::Token(token) | Self::Inline { token, .. } => token,
        }
    }
}

/// One use of a known flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagUse {
    /// The canonical long name (`select` for `-s`, `resource-type` for `--resource-types`).
    pub name: String,
    /// Index of the token that names the flag.
    pub token: usize,
    /// The command path the flag was read in (`[]` before the command).
    pub scope: Vec<String>,
    pub values: Vec<ValueSlot>,
}

/// What an argv means to dbt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scan {
    /// The (sub)command path, e.g. `["source", "freshness"]`. Empty when no command was given.
    pub path: Vec<String>,
    pub flags: Vec<FlagUse>,
    /// Positional arguments (the macro of `run-operation`, unknown commands, ...).
    pub positionals: Vec<usize>,
    /// Flags the grammar does not know. Whether they take a value is unknown, so a scan with unknown flags
    /// may have misplaced what follows them.
    pub unknown: Vec<usize>,
    /// Index of a `--` terminator.
    pub terminator: Option<usize>,
}

impl Scan {
    pub fn command(&self) -> Option<&str> {
        self.path.first().map(String::as_str)
    }

    pub fn uses(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f.name == name)
    }

    pub fn uses_any(&self, names: &[String]) -> bool {
        names.iter().any(|n| self.uses(n))
    }

    /// The values given to a flag, last use last.
    pub fn values(&self, args: &[OsString], name: &str) -> Vec<String> {
        self.flags
            .iter()
            .filter(|f| f.name == name)
            .flat_map(|f| f.values.iter().map(|slot| slot_text(args, *slot)))
            .collect()
    }

    /// The value of a single-valued flag (the last use wins, as in clap's override behaviour).
    pub fn value(&self, args: &[OsString], name: &str) -> Option<String> {
        self.values(args, name).pop()
    }

    /// Whether the scan is certain: no unknown flags before (or inside) the command path.
    pub fn is_certain(&self) -> bool {
        self.unknown.is_empty()
    }
}

/// The text of a value slot (lossy for non-UTF-8 tokens).
pub fn slot_text(args: &[OsString], slot: ValueSlot) -> String {
    match slot {
        ValueSlot::Token(token) => args[token].to_string_lossy().into_owned(),
        ValueSlot::Inline { token, offset } => {
            let text = args[token].to_string_lossy();
            text.get(offset..).unwrap_or_default().to_owned()
        }
    }
}

/// Whether clap would read `token` as a flag (and so not as a value).
fn looks_like_flag(token: &str) -> bool {
    token.starts_with('-') && token.len() > 1 && !is_negative_number(token)
}

fn is_negative_number(token: &str) -> bool {
    token
        .strip_prefix('-')
        .is_some_and(|rest| !rest.is_empty() && rest.parse::<f64>().is_ok())
}

/// Scans `args` (without the program name) against the grammar.
pub fn scan(args: &[OsString], grammar: &Grammar) -> Scan {
    let mut out = Scan::default();
    let mut scope: &Scope = &grammar.root;
    let mut i = 0;
    while i < args.len() {
        let Some(token) = args[i].to_str() else {
            // Not UTF-8: never a flag dbt knows; a positional or a value (paths).
            out.positionals.push(i);
            i += 1;
            continue;
        };
        if out.terminator.is_some() {
            out.positionals.push(i);
            i += 1;
            continue;
        }
        if token == "--" {
            out.terminator = Some(i);
            i += 1;
            continue;
        }
        if let Some(body) = token.strip_prefix("--") {
            let (name, inline) = match body.split_once('=') {
                Some((name, _)) => (name, Some(2 + name.len() + 1)),
                None => (body, None),
            };
            match grammar.long(scope, name) {
                None => out.unknown.push(i),
                Some(spec) => {
                    let mut flag = FlagUse {
                        name: spec.long.clone(),
                        token: i,
                        scope: out.path.clone(),
                        values: Vec::new(),
                    };
                    match (spec.arity, inline) {
                        (Arity::None, _) => {}
                        (_, Some(offset)) => {
                            flag.values.push(ValueSlot::Inline { token: i, offset });
                        }
                        (arity, None) => i = take_values(args, i, arity, &mut flag.values),
                    }
                    out.flags.push(flag);
                }
            }
            i += 1;
            continue;
        }
        if looks_like_flag(token) {
            // A cluster of short flags: `-qd`, `-sx`, `-s=x`, `-qs x`.
            let mut offset = 1;
            for c in token[1..].chars() {
                offset += c.len_utf8();
                let Some(spec) = scope.short(c) else {
                    out.unknown.push(i);
                    break;
                };
                let mut flag = FlagUse {
                    name: spec.long.clone(),
                    token: i,
                    scope: out.path.clone(),
                    values: Vec::new(),
                };
                if spec.arity == Arity::None {
                    out.flags.push(flag);
                    continue;
                }
                let rest = &token[offset..];
                if rest.is_empty() {
                    i = take_values(args, i, spec.arity, &mut flag.values);
                } else {
                    // clap strips one `=` after a short flag (`-t=dev`).
                    let skip = usize::from(rest.starts_with('='));
                    flag.values.push(ValueSlot::Inline {
                        token: i,
                        offset: offset + skip,
                    });
                }
                out.flags.push(flag);
                break;
            }
            i += 1;
            continue;
        }
        match scope.command(token) {
            Some(child) => {
                out.path.push(token.to_owned());
                scope = child;
            }
            None => out.positionals.push(i),
        }
        i += 1;
    }
    out
}

/// Consumes the values that follow a flag at `i`; returns the index of the last token consumed.
fn take_values(args: &[OsString], i: usize, arity: Arity, values: &mut Vec<ValueSlot>) -> usize {
    let mut last = i;
    let is_value = |index: usize| {
        args.get(index).is_some_and(|t| match t.to_str() {
            Some(text) => text != "--" && !looks_like_flag(text),
            None => true,
        })
    };
    match arity {
        Arity::None => {}
        Arity::Single => {
            if is_value(i + 1) {
                values.push(ValueSlot::Token(i + 1));
                last = i + 1;
            }
        }
        Arity::Multi | Arity::OptionalMulti => {
            while is_value(last + 1) {
                last += 1;
                values.push(ValueSlot::Token(last));
            }
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn run(words: &[&str]) -> (Scan, Vec<OsString>) {
        let argv = args(words);
        (scan(&argv, &Grammar::embedded()), argv)
    }

    #[test]
    fn finds_the_command_after_global_flags() {
        let (scan, _) = run(&["--debug", "--target", "build", "build"]);
        assert_eq!(scan.path, ["build"]);
        assert!(scan.uses("debug"));
        assert!(!scan.uses("select"));
    }

    #[test]
    fn select_consumes_until_the_next_flag_even_command_names() {
        let (scan, argv) = run(&["--select", "a", "b", "build"]);
        assert!(scan.path.is_empty(), "{scan:?}");
        assert_eq!(scan.values(&argv, "select"), ["a", "b", "build"]);
    }

    #[test]
    fn inline_and_clustered_values() {
        let (scan, argv) = run(&["build", "-qsa+", "--vars={a: 1}", "-t=dev"]);
        assert_eq!(scan.path, ["build"]);
        assert!(scan.uses("quiet"));
        assert_eq!(scan.values(&argv, "select"), ["a+"]);
        assert_eq!(scan.value(&argv, "vars").as_deref(), Some("{a: 1}"));
        assert_eq!(scan.value(&argv, "target").as_deref(), Some("dev"));
    }

    #[test]
    fn after_the_terminator_everything_is_positional() {
        let (scan, _) = run(&["build", "--", "--select", "x"]);
        assert!(!scan.uses("select"));
        assert_eq!(scan.positionals, [2, 3]);
    }

    #[test]
    fn nested_subcommands_and_aliases() {
        let (scan, argv) = run(&["source", "freshness", "--resource-types", "source"]);
        assert_eq!(scan.path, ["source", "freshness"]);
        let (scan2, argv2) = run(&["build", "--resource-types", "model", "seed"]);
        assert_eq!(scan2.values(&argv2, "resource-type"), ["model", "seed"]);
        let _ = (scan, argv);
    }

    #[test]
    fn unknown_flags_are_recorded() {
        let (scan, _) = run(&["--no-such-flag", "build"]);
        assert_eq!(scan.unknown, [0]);
        assert_eq!(scan.path, ["build"]);
        assert!(!scan.is_certain());
    }
}
