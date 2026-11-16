//! Derives a [`Grammar`] from a dbt binary.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::DbtError;
use crate::flags::{Arity, FlagSpec, Grammar, HIDDEN_ALIASES, Scope};

/// Runs `dbt completions fish` and `dbt <path> --help` for every scope.
pub fn from_binary(binary: &Path) -> Result<Grammar, DbtError> {
    let version = run(binary, ["--version"])?;
    let fish = run(binary, ["completions", "fish"])?;
    let mut grammar = from_fish(&fish)?;
    version.trim().clone_into(&mut grammar.dbt_version);
    // Something that answers `--version` but is not dbt v2 (a stub, a wrapper script) must not yield an empty
    // grammar: every command would then look unknown and nothing would be guarded.
    for required in ["build", "run", "run-operation", "parse"] {
        if grammar.root.command(required).is_none() {
            return Err(DbtError::Grammar(format!(
                "{} lists no `{required}` command in its completions",
                binary.display()
            )));
        }
    }
    if grammar.root.long("select").is_none() {
        return Err(DbtError::Grammar(format!(
            "{} lists no --select flag in its completions",
            binary.display()
        )));
    }
    for path in scope_paths(&grammar.root, &mut Vec::new()) {
        let mut args: Vec<&str> = path.iter().map(String::as_str).collect();
        args.push("--help");
        let help = run(binary, &args)?;
        let scope = scope_mut(&mut grammar.root, &path);
        apply_help(scope, &help);
    }
    inherit_root_arity(&mut grammar.root);
    Ok(grammar)
}

/// `dbt --help` lists no options, so the root learns multi-valued flags from its subcommands: dbt's global
/// flags (`global = true`) are one definition shared by every subcommand.
fn inherit_root_arity(root: &mut Scope) {
    let multi: BTreeMap<String, Arity> = root
        .commands
        .values()
        .flat_map(|scope| scope.flags.iter())
        .filter(|f| matches!(f.arity, Arity::Multi | Arity::OptionalMulti))
        .map(|f| (f.long.clone(), f.arity))
        .collect();
    for flag in &mut root.flags {
        if flag.arity == Arity::Single
            && let Some(arity) = multi.get(&flag.long)
        {
            flag.arity = *arity;
        }
    }
}

fn run<I, S>(binary: &Path, args: I) -> Result<String, DbtError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<S> = args.into_iter().collect();
    let output = Command::new(binary)
        .args(&args)
        .env("NO_COLOR", "1")
        .output()
        .map_err(|source| DbtError::Spawn {
            binary: binary.to_owned(),
            source,
        })?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() && !text.trim().is_empty() {
        Ok(text)
    } else {
        let shown: Vec<String> = args
            .iter()
            .map(|a| a.as_ref().to_string_lossy().into_owned())
            .collect();
        Err(DbtError::Grammar(format!(
            "`dbt {}` failed: {}",
            shown.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn scope_paths(scope: &Scope, prefix: &mut Vec<String>) -> Vec<Vec<String>> {
    let mut paths = vec![prefix.clone()];
    for (name, child) in &scope.commands {
        if name == "help" {
            continue;
        }
        prefix.push(name.clone());
        paths.extend(scope_paths(child, prefix));
        prefix.pop();
    }
    paths
}

fn scope_mut<'a>(root: &'a mut Scope, path: &[String]) -> &'a mut Scope {
    path.iter().fold(root, |scope, name| {
        scope
            .commands
            .get_mut(name)
            .expect("path comes from the tree")
    })
}

/// Parses clap's fish completion script.
pub fn from_fish(script: &str) -> Result<Grammar, DbtError> {
    let mut root = Scope::default();
    for line in logical_lines(script) {
        let line = line.trim();
        if !line.starts_with("complete -c dbt ") {
            continue;
        }
        let words = fish_words(line)?;
        let Some(entry) = Entry::parse(&words) else {
            continue;
        };
        let Some(path) = condition_path(&entry.condition) else {
            continue;
        };
        if path.first().is_some_and(|c| c == "help") {
            continue;
        }
        let scope = ensure_scope(&mut root, &path);
        match (entry.long, entry.short) {
            (Some(long), short) => {
                if scope.long(&long).is_none() {
                    scope.flags.push(FlagSpec {
                        long,
                        short,
                        arity: if entry.requires_value {
                            Arity::Single
                        } else {
                            Arity::None
                        },
                    });
                }
            }
            (None, Some(_)) => {}
            (None, None) => {
                // `-f -a "build"` under "needs command" / "not seen subcommand": a subcommand name.
                if let Some(name) = entry.argument
                    && is_subcommand_listing(&entry.condition)
                {
                    scope.commands.entry(name).or_default();
                }
            }
        }
    }
    Ok(Grammar {
        dbt_version: String::new(),
        aliases: HIDDEN_ALIASES
            .iter()
            .filter(|(alias, canonical)| alias != canonical)
            .map(|(a, c)| ((*a).to_owned(), (*c).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        root,
    })
}

/// The `complete` entries, with physical lines joined while a quote is open (value lists in `-a "..."` span
/// lines). Everything else (comments, helper functions) is skipped: it is not ours to balance.
fn logical_lines(script: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    for line in script.lines() {
        if pending.is_empty() && !line.trim_start().starts_with("complete -c dbt ") {
            continue;
        }
        if !pending.is_empty() {
            pending.push('\n');
        }
        pending.push_str(line);
        if fish_words(&pending).is_ok() {
            lines.push(std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() {
        lines.push(pending);
    }
    lines
}

fn ensure_scope<'a>(root: &'a mut Scope, path: &[String]) -> &'a mut Scope {
    path.iter().fold(root, |scope, name| {
        scope.commands.entry(name.clone()).or_default()
    })
}

fn is_subcommand_listing(condition: &str) -> bool {
    condition == "__fish_dbt_needs_command"
        || condition.contains("and not __fish_seen_subcommand_from")
}

/// `__fish_dbt_needs_command` → `[]`; `__fish_dbt_using_subcommand source; and __fish_seen_subcommand_from
/// freshness` → `["source", "freshness"]`; the `and not ...` form is the parent command itself.
fn condition_path(condition: &str) -> Option<Vec<String>> {
    if condition == "__fish_dbt_needs_command" {
        return Some(Vec::new());
    }
    let mut parts = condition.split(';').map(str::trim);
    let command = parts.next()?.strip_prefix("__fish_dbt_using_subcommand ")?;
    let mut path = vec![command.trim().to_owned()];
    for part in parts {
        if let Some(seen) = part.strip_prefix("and __fish_seen_subcommand_from ") {
            path.push(seen.trim().to_owned());
        } else if !part.starts_with("and not __fish_seen_subcommand_from") {
            return None;
        }
    }
    Some(path)
}

#[derive(Debug, Default)]
struct Entry {
    condition: String,
    long: Option<String>,
    short: Option<char>,
    requires_value: bool,
    argument: Option<String>,
}

impl Entry {
    fn parse(words: &[String]) -> Option<Self> {
        let mut entry = Self::default();
        let mut i = 3; // complete -c dbt
        while i < words.len() {
            match words[i].as_str() {
                "-n" => {
                    entry.condition.clone_from(words.get(i + 1)?);
                    i += 1;
                }
                "-l" => {
                    entry.long = Some(words.get(i + 1)?.clone());
                    i += 1;
                }
                "-s" => {
                    entry.short = words.get(i + 1)?.chars().next();
                    i += 1;
                }
                "-a" => {
                    entry.argument = Some(words.get(i + 1)?.clone());
                    i += 1;
                }
                "-d" => i += 1,
                "-r" => entry.requires_value = true,
                _ => {}
            }
            i += 1;
        }
        Some(entry)
    }
}

/// Splits a fish command line into words: single quotes (with `\'` and `\\`), double quotes, backslashes.
fn fish_words(line: &str) -> Result<Vec<String>, DbtError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\\') => match chars.peek() {
                            Some('\'' | '\\') => word.push(chars.next().unwrap_or('\\')),
                            _ => word.push('\\'),
                        },
                        Some('\'') => break,
                        Some(other) => word.push(other),
                        None => {
                            return Err(DbtError::Grammar(format!("unterminated quote: {line}")));
                        }
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\\') => match chars.peek() {
                            Some('"' | '\\' | '$') => word.push(chars.next().unwrap_or('\\')),
                            _ => word.push('\\'),
                        },
                        Some('"') => break,
                        Some(other) => word.push(other),
                        None => {
                            return Err(DbtError::Grammar(format!("unterminated quote: {line}")));
                        }
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// Marks multi-valued flags from clap's long help: `--select <SELECT>...` takes one or more values,
/// `--show [<SHOW>...]` zero or more.
pub fn apply_help(scope: &mut Scope, help: &str) {
    for line in help.lines() {
        let line = line.trim_start();
        if !line.starts_with('-') {
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(mut first) = words.next() else {
            continue;
        };
        if first.len() == 3 && first.ends_with(',') && !first.starts_with("--") {
            // `-s,` then the long form.
            match words.next() {
                Some(next) => first = next,
                None => continue,
            }
        }
        let Some(long) = first.strip_prefix("--") else {
            continue;
        };
        let (long, inline_value) = match long.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (long, None),
        };
        let value = inline_value.or_else(|| words.next());
        let Some(value) = value else { continue };
        let arity = if value.starts_with("[<") && value.ends_with("...]") {
            Arity::OptionalMulti
        } else if value.starts_with('<') && value.ends_with("...") {
            Arity::Multi
        } else {
            continue;
        };
        if let Some(flag) = scope.flags.iter_mut().find(|f| f.long == long) {
            flag.arity = arity;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FISH: &str = r#"
complete -c dbt -n "__fish_dbt_needs_command" -s t -l target -d 'The target to execute' -r
complete -c dbt -n "__fish_dbt_needs_command" -s s -l select -d 'Select nodes to run' -r
complete -c dbt -n "__fish_dbt_needs_command" -s d -l debug -d 'Debug, it\'s "noisy"'
complete -c dbt -n "__fish_dbt_needs_command" -f -a "build" -d 'Build seeds, models and tests'
complete -c dbt -n "__fish_dbt_needs_command" -f -a "source" -d 'Run sources subcommands'
complete -c dbt -n "__fish_dbt_using_subcommand build" -s s -l select -d 'Select nodes to run' -r
complete -c dbt -n "__fish_dbt_using_subcommand build" -l show -d 'Show produced artifacts [default: \'progress\'] ' -r -f -a "progress\t''
nodes\t''"
complete -c dbt -n "__fish_dbt_using_subcommand source; and not __fish_seen_subcommand_from freshness help" -l vars -r
complete -c dbt -n "__fish_dbt_using_subcommand source; and not __fish_seen_subcommand_from freshness help" -f -a "freshness" -d 'Check'
complete -c dbt -n "__fish_dbt_using_subcommand source; and __fish_seen_subcommand_from freshness" -s o -l output -r
complete -c dbt -n "__fish_dbt_using_subcommand help; and __fish_seen_subcommand_from source" -f -a "freshness"
"#;

    #[test]
    fn reads_scopes_flags_and_subcommands_from_fish_completions() {
        let grammar = from_fish(FISH).unwrap();
        let root = &grammar.root;
        assert_eq!(root.short('t').unwrap().arity, Arity::Single);
        assert_eq!(root.long("debug").unwrap().arity, Arity::None);
        assert!(root.command("build").is_some());
        assert!(root.command("help").is_none());
        let source = root.command("source").unwrap();
        assert!(source.long("vars").is_some());
        let freshness = source.command("freshness").unwrap();
        assert_eq!(freshness.short('o').unwrap().long, "output");
        assert_eq!(grammar.aliases["resource-types"], "resource-type");
    }

    #[test]
    fn help_marks_multi_valued_flags() {
        let mut grammar = from_fish(FISH).unwrap();
        let build = grammar.root.commands.get_mut("build").unwrap();
        apply_help(
            build,
            "Options:\n  -s, --select <SELECT>...\n          Select nodes\n      --show [<SHOW>...]\n          Show\n",
        );
        assert_eq!(build.long("select").unwrap().arity, Arity::Multi);
        assert_eq!(build.long("show").unwrap().arity, Arity::OptionalMulti);
    }

    #[test]
    fn fish_quoting() {
        assert_eq!(
            fish_words(r#"a 'it\'s' "say \"x\"" b\ c"#).unwrap(),
            ["a", "it's", "say \"x\"", "b c"]
        );
    }
}
