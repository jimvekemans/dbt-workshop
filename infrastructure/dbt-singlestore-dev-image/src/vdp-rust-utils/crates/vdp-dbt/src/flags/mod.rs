//! dbt's command-line grammar: which subcommands exist and which flags take values.
//!
//! The wrapper must never misread an invocation, so the grammar is not written by hand. It is derived from the
//! dbt binary itself ([`generate`]): `dbt completions fish` lists every flag of every (sub)command, including
//! hidden ones, and whether it takes a value; `dbt <command> --help` shows which flags take several values
//! (`<SELECT>...`). A copy generated from `/opt/dbt` is embedded ([`Grammar::embedded`]) as the fallback; normally
//! the grammar of the installed engine is used and cached per binary ([`cache`]).

pub mod cache;
pub mod generate;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How many values a flag takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arity {
    /// A switch (`--debug`).
    None,
    /// Exactly one value (`--target dev`).
    Single,
    /// One or more values, up to the next flag (`--select a b c`).
    Multi,
    /// Zero or more values, up to the next flag (`--show`, `--show progress nodes`).
    OptionalMulti,
}

/// One flag of one (sub)command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagSpec {
    pub long: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<char>,
    pub arity: Arity,
}

/// The flags and subcommands available at one level (`dbt`, `dbt source`, `dbt source freshness`, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    #[serde(default)]
    pub flags: Vec<FlagSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub commands: BTreeMap<String, Scope>,
}

impl Scope {
    pub fn long(&self, name: &str) -> Option<&FlagSpec> {
        self.flags.iter().find(|f| f.long == name)
    }

    pub fn short(&self, c: char) -> Option<&FlagSpec> {
        self.flags.iter().find(|f| f.short == Some(c))
    }

    pub fn command(&self, name: &str) -> Option<&Scope> {
        self.commands.get(name)
    }
}

/// The whole grammar of one dbt binary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grammar {
    /// `dbt --version` of the binary it came from.
    pub dbt_version: String,
    /// Hidden long aliases (not visible in completions or help), alias → flag.
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    pub root: Scope,
}

/// Hidden aliases in `/opt/dbt/crates/dbt-clap-core/src/lib.rs` (`alias = ...` / `aliases = [...]`), which neither
/// completions nor help show. A test checks this list against the dbt sources when `VDP_DBT_SRC` is set.
pub const HIDDEN_ALIASES: &[(&str, &str)] = &[
    ("exclude-resource-types", "exclude-resource-type"),
    ("format", "output"),
    ("fusion-upgrade", "v2-upgrade"),
    ("resource-types", "resource-type"),
    ("run-cache-mode", "state-mode"),
    ("use-index", "write-index"),
    ("with-sample", "with-sample"),
];

impl Grammar {
    /// The grammar generated from the `/opt/dbt` build this toolkit was developed against.
    pub fn embedded() -> Self {
        serde_json::from_str(include_str!("../../grammar/dbt.json"))
            .expect("the embedded grammar is valid JSON")
    }

    /// The scope reached by a command path (`["source", "freshness"]`).
    pub fn scope(&self, path: &[String]) -> Option<&Scope> {
        path.iter()
            .try_fold(&self.root, |scope, name| scope.command(name))
    }

    /// Resolves a long flag in a scope, following hidden aliases.
    pub fn long<'a>(&'a self, scope: &'a Scope, name: &str) -> Option<&'a FlagSpec> {
        scope.long(name).or_else(|| {
            self.aliases
                .get(name)
                .and_then(|canonical| scope.long(canonical))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_grammar_knows_the_flags_the_guard_depends_on() {
        let grammar = Grammar::embedded();
        let build = grammar.scope(&["build".into()]).unwrap();
        assert_eq!(build.long("select").unwrap().arity, Arity::Multi);
        assert_eq!(build.short('s').unwrap().long, "select");
        assert_eq!(build.long("selector").unwrap().arity, Arity::Single);
        assert_eq!(build.long("debug").unwrap().arity, Arity::None);
        assert_eq!(
            grammar.long(build, "resource-types").unwrap().long,
            "resource-type"
        );
        let root = &grammar.root;
        assert_eq!(root.long("vars").unwrap().arity, Arity::Single);
        assert_eq!(root.short('t').unwrap().long, "target");
        assert!(root.command("run-operation").is_some());
        let run_operation = grammar.scope(&["run-operation".into()]).unwrap();
        assert_eq!(run_operation.long("args").unwrap().arity, Arity::Single);
        assert!(
            grammar
                .scope(&["source".into(), "freshness".into()])
                .is_some()
        );
    }
}
