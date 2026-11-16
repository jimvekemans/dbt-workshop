//! Undefined sources reported by dbt (`DependencyNotFound`, dbt1048).
//!
//! dbt v2 reports `Source '<source>.<table>' not found in project. Searched for '...'`
//! (`/opt/dbt/crates/dbt-jinja-utils/src/node_resolver.rs`), followed by `  --> <file>:<line>:<col>`. The same
//! text appears on the console, in `logs/dbt.log` and in the `msg` of `--log-format json` events (code `1048`).

/// One undefined `source('<source>', '<table>')`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissingSource {
    pub source: String,
    pub table: String,
    /// `models/x.sql:1:15`, when dbt said where.
    pub location: Option<String>,
}

impl MissingSource {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.source, self.table)
    }
}

const MARKER: &str = "(dbt1048)]: Source '";

/// Every undefined source in console text, `dbt.log` text or `--log-format json` lines, deduplicated by name
/// (first location wins), in order of appearance.
pub fn missing_sources(text: &str) -> Vec<MissingSource> {
    let mut found: Vec<MissingSource> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let message;
        let (body, following) = if line.trim_start().starts_with('{') {
            match json_message(line) {
                Some(msg) => {
                    message = msg;
                    let mut parts = message.splitn(2, '\n');
                    (parts.next().unwrap_or_default(), parts.next())
                }
                None => continue,
            }
        } else {
            (*line, lines.get(index + 1).copied())
        };
        let Some(source) = parse_line(body) else {
            continue;
        };
        let location = following
            .and_then(|next| next.trim().strip_prefix("--> "))
            .map(|loc| loc.trim().to_owned());
        if !found
            .iter()
            .any(|f| f.source == source.0 && f.table == source.1)
        {
            found.push(MissingSource {
                source: source.0,
                table: source.1,
                location,
            });
        }
    }
    found
}

fn json_message(line: &str) -> Option<String> {
    let event: serde_json::Value = serde_json::from_str(line).ok()?;
    let info = event.get("info")?;
    (info.get("code")?.as_str()? == "1048").then(|| info.get("msg")?.as_str().map(str::to_owned))?
}

fn parse_line(line: &str) -> Option<(String, String)> {
    let start = line.find(MARKER)? + MARKER.len();
    let rest = &line[start..];
    let end = rest.find("' not found")?;
    let (source, table) = rest[..end].split_once('.')?;
    Some((source.to_owned(), table.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONSOLE: &str = "\
[error] [DependencyNotFound (dbt1048)]: Source 'raw_src.new_table' not found in project. Searched for 'mini.raw_src.new_table, raw_src.new_table'
  --> models/m2.sql:1:15
[error] [DependencyNotFound (dbt1048)]: Ref 'nope' not found in project. Searched for 'mini.nope, nope'
  --> models/m3.sql:1:53
[error] [DependencyNotFound (dbt1048)]: Source 'raw_src.other' not found in project. Searched for 'mini.raw_src.other, raw_src.other'
  --> models/m3.sql:1:15
";

    #[test]
    fn reads_console_and_log_text() {
        let found = missing_sources(CONSOLE);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].qualified(), "raw_src.new_table");
        assert_eq!(found[0].location.as_deref(), Some("models/m2.sql:1:15"));
        assert_eq!(found[1].table, "other");
        let log = "08:25:21.852073 [error]: [DependencyNotFound (dbt1048)]: Source 'a.b' not found in project. Searched for 'p.a.b, a.b'\n";
        assert_eq!(missing_sources(log)[0].qualified(), "a.b");
    }

    #[test]
    fn reads_json_log_events() {
        let line = r#"{"data":{},"info":{"code":"1048","level":"error","msg":"[error] [DependencyNotFound (dbt1048)]: Source 'raw_src.new_table' not found in project. Searched for 'x'\n  --> models/m2.sql:1:15","name":"DependencyNotFound"}}"#;
        let found = missing_sources(line);
        assert_eq!(found[0].qualified(), "raw_src.new_table");
        assert_eq!(found[0].location.as_deref(), Some("models/m2.sql:1:15"));
    }
}
