//! The configuration key tables (`README.md`, `.agents/skills/vdp-rust-utils/references/config.md`) against the `*Settings`
//! structs: same sections, same keys, same defaults. Edit a struct, its `Default` or a table and this test says which of
//! them disagrees.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;
use vdp_cli::commands::MODULE_SECTIONS;
use vdp_core::{CoreSettings, EnvironmentSettings, SecretsSettings};

const START: &str = "<!-- config-tables:start -->";
const END: &str = "<!-- config-tables:end -->";

/// Sections of the sibling binaries: `vdp` claims them (so it does not warn) but documents none of them.
const SIBLING_SECTIONS: &[&str] = &["vaultspeed", "vaultspeed_release", "release_hub", "notify"];

fn read_block(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (_, rest) = text
        .split_once(START)
        .unwrap_or_else(|| panic!("{relative}: no {START}"));
    let (block, _) = rest
        .split_once(END)
        .unwrap_or_else(|| panic!("{relative}: no {END}"));
    block.trim().to_owned()
}

fn readme() -> String {
    read_block("README.md")
}

/// One documented key: its name and the text of its Default column (`None` for a table without one).
struct Row {
    key: String,
    default: Option<String>,
}

/// One documented section: `[heading]`, the settings type named after it, and its rows.
struct Section {
    heading: String,
    type_name: String,
    rows: Vec<Row>,
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split(" | ")
        .map(|c| c.trim().to_owned())
        .collect()
}

fn unquote(cell: &str) -> &str {
    cell.trim_matches('`')
}

fn parse(block: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut in_table = false;
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("**`[") {
            let (heading, rest) = rest.split_once("]`**").expect("heading end");
            let type_name = rest
                .trim_start_matches(" (`")
                .split(['`', ';'])
                .next()
                .unwrap()
                .to_owned();
            sections.push(Section {
                heading: heading.to_owned(),
                type_name,
                rows: Vec::new(),
            });
            in_table = false;
        } else if line.starts_with('|') {
            let section = sections.last_mut().expect("a table without a heading");
            let columns = cells(line);
            if columns[0] == "Key" {
                in_table = true; // the header row; the next row is the separator
            } else if in_table && !columns[0].starts_with("---") {
                // Key | Type | Default | Meaning, or Key | Type | Meaning (no defaults)
                let default = (columns.len() == 4).then(|| columns[2].clone());
                section.rows.push(Row {
                    key: unquote(&columns[0]).to_owned(),
                    default,
                });
            }
        }
    }
    sections
}

/// A value as the tables write it (TOML).
fn literal(value: &Value) -> String {
    match value {
        Value::String(s) => format!("{s:?}"),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(literal).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(map) if map.is_empty() => "{}".into(),
        Value::Object(map) => format!(
            "{{ {} }}",
            map.iter()
                .map(|(k, v)| format!("{k} = {}", literal(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

/// What the code says about a section: its settings type and the serialised defaults (several for a section whose
/// defaults depend on the environment).
struct Expected {
    heading: &'static str,
    type_name: &'static str,
    defaults: Vec<Value>,
}

fn expected<T: serde::Serialize>(heading: &'static str, defaults: &[T]) -> Expected {
    let full = std::any::type_name::<T>();
    Expected {
        heading,
        type_name: full.rsplit("::").next().unwrap(),
        defaults: defaults
            .iter()
            .map(|d| serde_json::to_value(d).unwrap())
            .collect(),
    }
}

/// The sections of the modules compiled into this test, and the headings of those that are not.
fn from_code() -> (Vec<Expected>, Vec<&'static str>) {
    #[allow(unused_mut)] // every module feature off: nothing is added
    let mut sections = vec![
        expected("core", &[CoreSettings::default()]),
        expected("secrets", &[SecretsSettings::default()]),
        expected(
            "environments.<name>",
            &[
                EnvironmentSettings::non_production(),
                EnvironmentSettings::production(),
            ],
        ),
        expected("dbt", &[vdp_dbt::DbtSettings::default()]),
    ];
    #[cfg(feature = "sources")]
    {
        sections.push(expected(
            "sources",
            &[vdp_sources::SourcesSettings::default()],
        ));
        sections.push(expected(
            "sources.database_prefix",
            &[vdp_sources::DatabasePrefix {
                physical: String::new(),
                jinja: String::new(),
            }],
        ));
    }
    #[cfg(feature = "release")]
    {
        sections.push(expected("git", &[vdp_git::GitSettings::default()]));
        sections.push(expected("jira", &[vdp_jira::JiraSettings::default()]));
        sections.push(expected(
            "release",
            &[vdp_release::ReleaseSettings::default()],
        ));
    }
    #[cfg(feature = "qa")]
    sections.push(expected("qa", &[vdp_qa::QaSettings::default()]));
    let compiled_out = vec![
        #[cfg(not(feature = "sources"))]
        "sources",
        #[cfg(not(feature = "sources"))]
        "sources.database_prefix",
        #[cfg(not(feature = "release"))]
        "git",
        #[cfg(not(feature = "release"))]
        "jira",
        #[cfg(not(feature = "release"))]
        "release",
        #[cfg(not(feature = "qa"))]
        "qa",
    ];
    (sections, compiled_out)
}

#[test]
fn the_two_copies_of_the_key_tables_are_equal() {
    assert_eq!(
        readme(),
        read_block(".agents/skills/vdp-rust-utils/references/config.md"),
        "README.md and references/config.md must hold the same text between the config-tables comments"
    );
}

#[test]
fn the_key_tables_match_the_settings_structs() {
    let documented = parse(&readme());
    let (expected, compiled_out) = from_code();

    let headings: BTreeSet<&str> = documented.iter().map(|s| s.heading.as_str()).collect();
    let known: BTreeSet<&str> = expected
        .iter()
        .map(|e| e.heading)
        .chain(compiled_out.iter().copied())
        .collect();
    assert_eq!(
        headings, known,
        "the documented sections and the settings structs differ"
    );

    let by_heading: BTreeMap<&str, &Section> =
        documented.iter().map(|s| (s.heading.as_str(), s)).collect();
    for section in &expected {
        let doc = by_heading[section.heading];
        let at = format!("[{}]", section.heading);
        assert_eq!(
            doc.type_name, section.type_name,
            "{at}: the type named in the heading"
        );

        // Keys: the serialised default has them all (a key left out when unset is documented as `not set`).
        let in_code: BTreeSet<&str> = section.defaults[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let in_doc: BTreeSet<&str> = doc.rows.iter().map(|r| r.key.as_str()).collect();
        let unset: BTreeSet<&str> = doc
            .rows
            .iter()
            .filter(|r| r.default.as_deref().is_some_and(|d| d.contains("not set")))
            .map(|r| r.key.as_str())
            .collect();
        let missing_in_doc: Vec<_> = in_code.difference(&in_doc).collect();
        let missing_in_code: Vec<_> = in_doc
            .difference(&in_code)
            .filter(|k| !unset.contains(*k))
            .collect();
        assert!(
            missing_in_doc.is_empty() && missing_in_code.is_empty(),
            "{at}: keys only in the struct: {missing_in_doc:?}; keys only in the table: {missing_in_code:?}"
        );

        // Defaults.
        for row in &doc.rows {
            // A row without a Default column, or a key that is left out when unset (`not set`), has no value to compare.
            let Some(cell) = &row.default else { continue };
            if !section.defaults[0]
                .as_object()
                .unwrap()
                .contains_key(&row.key)
            {
                continue;
            }
            if section.defaults.len() == 1 {
                let want = literal(&section.defaults[0][&row.key]);
                assert!(
                    unquote(cell) == want,
                    "{at} {}: the table says {cell}, the struct's default is {want}",
                    row.key
                );
            } else {
                for default in &section.defaults {
                    let want = literal(&default[&row.key]);
                    assert!(
                        cell.contains(&want),
                        "{at} {}: the table says {cell}, which lacks the default {want}",
                        row.key
                    );
                }
            }
        }
    }
}

#[test]
fn the_documented_sections_are_the_ones_the_toolkit_claims() {
    let mut documented: BTreeSet<String> = parse(&readme())
        .iter()
        .map(|s| s.heading.split('.').next().unwrap().to_owned())
        .collect();
    let claimed: BTreeSet<String> = MODULE_SECTIONS.iter().map(|s| (*s).to_owned()).collect();
    // `core`, `secrets` and `environments` belong to vdp-core, which always reads them.
    for core in ["core", "secrets", "environments"] {
        assert!(documented.remove(core), "[{core}] is not documented");
    }
    let sibling: BTreeSet<String> = SIBLING_SECTIONS.iter().map(|s| (*s).to_owned()).collect();
    let toolkit: BTreeSet<String> = claimed.difference(&sibling).cloned().collect();
    assert_eq!(
        documented, toolkit,
        "documented module sections and `commands::MODULE_SECTIONS` (without the sibling binaries' sections) differ"
    );
}
