//! `--vars` and `--args` values, read exactly as dbt v2 reads them.
//!
//! dbt parses both with `check_key_value_cli_arg` (`/opt/dbt/crates/dbt-common/src/io_args.rs`): trim, strip
//! outer `'`, then YAML (`dbt-yaml`, the crate used here too) and JSON as fallback. For `--vars` (and only
//! `--vars`) dbt then recovers flow-style `{a:1}`, which YAML reads as the single key `a:1` with a null value.
//!
//! The rules here:
//! - A value dbt accepts keeps its exact bytes, so dbt's own reading (and its own `--vars` recovery) decides.
//!   The one exception is `--args`: dbt accepts `{a:1}` there but silently reads `{"a:1": null}`, so the wrapper
//!   applies dbt's own `--vars` recovery to it.
//! - A value dbt rejects is retried with the Python wrapper's repair (a space after every `:` that is followed by
//!   a non-space, now only outside quotes). If dbt accepts the repaired value, that is used: `{a:{b: 1}}` worked
//!   with the Python wrapper and keeps working. Otherwise the original value is forwarded and dbt reports it.
//! - A value that changes is emitted as JSON with `", "`/`": "` spacing, which dbt reads back as the same map.

use std::collections::BTreeMap;
use std::io;

use dbt_yaml::Value;
use serde::Serialize;

/// The map dbt reads from a `--vars`/`--args` value.
pub type Mapping = BTreeMap<String, Value>;

/// What happens to a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Fix {
    /// Forwarded byte for byte.
    Unchanged,
    /// dbt would misread it (keys with an unspaced colon); recovered as dbt recovers `--vars`.
    Recovered { value: String, keys: Vec<String> },
    /// dbt would reject it; repaired the way the Python wrapper did.
    Repaired { value: String },
}

/// `check_key_value_cli_arg` of dbt v2.
///
/// # Errors
///
/// A message when `value` is empty, or is neither a YAML nor a JSON mapping.
pub fn dbt_parse(value: &str) -> Result<Mapping, String> {
    if value.trim().is_empty() {
        return Err("Empty input is not valid".into());
    }
    let vars = value.trim().trim_matches('\'');
    match dbt_yaml::from_str::<Mapping>(vars) {
        Ok(map) => Ok(map),
        Err(_) => serde_json::from_str::<Mapping>(vars).map_err(|_| {
            "Invalid YAML/JSON format. Expected format: 'key: value' or '{key: value, ..}'"
                .to_owned()
        }),
    }
}

/// Whether dbt applies its flow-style recovery to this value.
fn is_flow_style(value: &str) -> bool {
    value.trim().trim_matches('\'').starts_with('{')
}

/// Keys dbt's `recover_unspaced_colon_pairs` would split.
fn ambiguous_keys(map: &Mapping) -> Vec<String> {
    map.iter()
        .filter(|(key, value)| key.contains(':') && value.is_null())
        .map(|(key, _)| key.clone())
        .collect()
}

/// `recover_unspaced_colon_pairs` of dbt v2, verbatim in behaviour.
pub fn recover_unspaced_colon_pairs(mut map: Mapping) -> Mapping {
    for full_key in ambiguous_keys(&map) {
        let Some((real_key, raw_value)) = full_key.split_once(':') else {
            continue;
        };
        let raw_value = raw_value.trim();
        let parsed =
            dbt_yaml::from_str::<Value>(raw_value).unwrap_or_else(|_| Value::from(raw_value));
        map.remove(&full_key);
        map.insert(real_key.to_owned(), parsed);
    }
    map
}

/// dbt's full reading of a `--vars` value (parse, then recovery for flow style).
///
/// # Errors
///
/// A message when [`dbt_parse`] rejects `value`.
pub fn dbt_read_vars(value: &str) -> Result<Mapping, String> {
    let map = dbt_parse(value)?;
    Ok(if is_flow_style(value) {
        recover_unspaced_colon_pairs(map)
    } else {
        map
    })
}

/// The Python wrapper's repair, limited to text outside quotes: a space after every `:` followed by a non-space.
pub fn python_repair(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('\'');
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    let mut out = String::with_capacity(trimmed.len() + 8);
    let mut quote: Option<char> = None;
    let mut chars = trimmed.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        match quote {
            Some('"') if c == '\\' => {
                if let Some(escaped) = chars.next() {
                    out.push(escaped);
                }
            }
            Some(q) if c == q => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ':' && chars.peek().is_some_and(|next| !next.is_whitespace()) => {
                out.push(' ');
            }
            Some(_) | None => {}
        }
    }
    (out != trimmed).then_some(out)
}

/// What to do with a `--vars` value.
pub fn fix_vars(value: &str) -> Fix {
    if dbt_parse(value).is_ok() {
        return Fix::Unchanged;
    }
    repair(value)
}

/// What to do with a `run-operation --args` value.
pub fn fix_args(value: &str) -> Fix {
    match dbt_parse(value) {
        Ok(map) => {
            let keys = ambiguous_keys(&map);
            if !is_flow_style(value) || keys.is_empty() {
                return Fix::Unchanged;
            }
            match to_cli_json(&recover_unspaced_colon_pairs(map)) {
                Some(value) => Fix::Recovered { value, keys },
                None => Fix::Unchanged,
            }
        }
        Err(_) => repair(value),
    }
}

fn repair(value: &str) -> Fix {
    let Some(repaired) = python_repair(value) else {
        return Fix::Unchanged;
    };
    match dbt_parse(&repaired) {
        Ok(map) if ambiguous_keys(&map).is_empty() => match to_cli_json(&map) {
            Some(value) => Fix::Repaired { value },
            None => Fix::Unchanged,
        },
        _ => Fix::Unchanged,
    }
}

/// JSON with `", "` and `": "`, so any YAML reader agrees with any JSON reader. `None` when the map holds values
/// JSON cannot carry (NaN, tagged values); the caller then forwards the original.
pub fn to_cli_json(map: &Mapping) -> Option<String> {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, Spaced);
    map.serialize(&mut serializer).ok()?;
    let text = String::from_utf8(out).ok()?;
    // Check the round trip through dbt's own reader before trusting it.
    (dbt_read_vars(&text).ok()? == *map).then_some(text)
}

struct Spaced;

impl serde_json::ser::Formatter for Spaced {
    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_dbt_accepts_are_never_touched() {
        for value in [
            "{a: 1}",
            "a: 1",
            "a: 1\nb: [x, y]",
            r#"{"a": {"b": [1, 2]}}"#,
            r#"{url: "http://h:8080/p?q=1"}"#,
            "{t: 12:30}",
            "{a:1}",
            "{a:1, b: 2}",
            "'{a: 1}'",
            "{}",
        ] {
            assert_eq!(fix_vars(value), Fix::Unchanged, "{value}");
        }
    }

    #[test]
    fn args_get_dbts_vars_recovery() {
        let Fix::Recovered { value, keys } = fix_args("{days:7, name: x}") else {
            panic!("expected recovery");
        };
        assert_eq!(keys, ["days:7"]);
        assert_eq!(value, r#"{"days": 7, "name": "x"}"#);
        assert_eq!(fix_args("{days: 7}"), Fix::Unchanged);
    }

    #[test]
    fn values_dbt_rejects_get_the_python_repair() {
        assert_eq!(
            fix_vars("{a:{b: 1}}"),
            Fix::Repaired {
                value: r#"{"a": {"b": 1}}"#.into()
            }
        );
        assert_eq!(
            fix_args("{a:{b:1}, c:[1,2]}"),
            Fix::Repaired {
                value: r#"{"a": {"b": 1}, "c": [1, 2]}"#.into()
            }
        );
        // Unrepairable input is forwarded so dbt reports it.
        assert_eq!(fix_vars("{a: [1, 2"), Fix::Unchanged);
        assert_eq!(fix_vars("not a map"), Fix::Unchanged);
    }

    #[test]
    fn the_repair_never_touches_quoted_text() {
        assert_eq!(
            python_repair(r#"{u:"http://x", v:'a:b'}"#).unwrap(),
            r#"{u: "http://x", v: 'a:b'}"#
        );
    }

    #[test]
    fn reads_scalars_exactly_as_dbt_v2() {
        // `eval_args.vars` recorded by dbt-oss 2.0.11 for this --vars value.
        let read = dbt_read_vars("{a: yes, b: 'no', c: 007, d: on, e: 0x1F, f: 1_000}").unwrap();
        assert_eq!(
            serde_json::to_string(&read).unwrap(),
            r#"{"a":"yes","b":"no","c":"007","d":"on","e":31,"f":1000}"#
        );
    }

    #[test]
    fn json_output_keeps_types() {
        let map = dbt_parse("{a: '007', b: 7, c: 'yes', d: yes, e: ~, f: 1.5}").unwrap();
        let json = to_cli_json(&map).unwrap();
        assert_eq!(dbt_read_vars(&json).unwrap(), map);
    }
}
