//! Reading and (minimally) preparing a dbt argv.

pub mod scan;
pub mod yaml_arg;

use std::ffi::OsString;

pub use scan::{FlagUse, Scan, ValueSlot, scan, slot_text};
pub use yaml_arg::Fix;

/// A change made to one value, for the user's information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub flag: String,
    pub before: String,
    pub after: String,
    pub reason: String,
}

/// Applies the `--args`/`--vars` fixes (see [`yaml_arg`]) and leaves every other byte alone.
pub fn prepare(args: &[OsString], scan: &Scan) -> (Vec<OsString>, Vec<Notice>) {
    let mut out = args.to_vec();
    let mut notices = Vec::new();
    for flag in &scan.flags {
        let fixer: fn(&str) -> Fix = match (flag.name.as_str(), scan.command()) {
            ("args", Some("run-operation")) => yaml_arg::fix_args,
            ("vars", _) => yaml_arg::fix_vars,
            _ => continue,
        };
        for slot in &flag.values {
            let Some(token) = args[slot.token()].to_str() else {
                continue;
            };
            let value = match *slot {
                ValueSlot::Token(_) => token,
                ValueSlot::Inline { offset, .. } => &token[offset..],
            };
            let (after, reason) = match fixer(value) {
                Fix::Unchanged => continue,
                Fix::Recovered { value, keys } => (
                    value,
                    format!(
                        "dbt reads {} as a key without a value; split at the first ':' as dbt does for --vars",
                        keys.join(", ")
                    ),
                ),
                Fix::Repaired { value } => (
                    value,
                    "dbt rejects it; added a space after ':' as the Python wrapper did".to_owned(),
                ),
            };
            out[slot.token()] = match *slot {
                ValueSlot::Token(_) => OsString::from(&after),
                ValueSlot::Inline { offset, .. } => {
                    OsString::from(format!("{}{after}", &token[..offset]))
                }
            };
            notices.push(Notice {
                flag: format!("--{}", flag.name),
                before: value.to_owned(),
                after,
                reason,
            });
        }
    }
    (out, notices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flags::Grammar;

    fn prepared(words: &[&str]) -> Vec<String> {
        let argv: Vec<OsString> = words.iter().map(OsString::from).collect();
        let scan = scan(&argv, &Grammar::embedded());
        prepare(&argv, &scan)
            .0
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn only_misread_values_change_and_positions_stay() {
        assert_eq!(
            prepared(&[
                "run-operation",
                "m",
                "--args",
                "{days:7}",
                "--vars",
                "{a:1}"
            ]),
            [
                "run-operation",
                "m",
                "--args",
                r#"{"days": 7}"#,
                "--vars",
                "{a:1}"
            ]
        );
        assert_eq!(
            prepared(&["run-operation", "m", "--args={days:7}"]),
            ["run-operation", "m", r#"--args={"days": 7}"#]
        );
        // `--args` of another command (none exists today) and values after `--` are left alone.
        assert_eq!(
            prepared(&["run-operation", "m", "--", "--args", "{x:1}"])[4],
            "{x:1}"
        );
    }
}
