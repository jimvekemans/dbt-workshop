use std::io::{self, Write};

use crate::ResultSet;

/// Writes a result set as RFC 4180 CSV with a header row. `NULL` becomes an empty cell.
///
/// # Errors
///
/// The [`io::Error`] of `out` when a write fails.
pub fn write_csv(result: &ResultSet, out: &mut dyn Write) -> io::Result<()> {
    write_record(out, result.columns.iter().map(String::as_str))?;
    for row in &result.rows {
        let cells: Vec<String> = row.iter().map(ToString::to_string).collect();
        write_record(out, cells.iter().map(String::as_str))?;
    }
    Ok(())
}

fn write_record<'a>(out: &mut dyn Write, cells: impl Iterator<Item = &'a str>) -> io::Result<()> {
    let line: Vec<String> = cells.map(quote).collect();
    writeln!(out, "{}", line.join(","))
}

fn quote(cell: &str) -> String {
    if cell.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqlValue;

    #[test]
    fn quotes_only_when_needed() {
        let result = ResultSet {
            columns: vec!["id".into(), "note".into()],
            rows: vec![
                vec![SqlValue::Int(1), SqlValue::Text("a,b".into())],
                vec![SqlValue::Null, SqlValue::Text("say \"hi\"".into())],
            ],
        };
        let mut out = Vec::new();
        write_csv(&result, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "id,note\n1,\"a,b\"\n,\"say \"\"hi\"\"\"\n"
        );
    }
}
