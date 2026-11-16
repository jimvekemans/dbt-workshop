use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Mutex;

use regex::Regex;
use serde::{Deserialize, Serialize};
use vdp_git::Vcs;
use vdp_vaultspeed::sources::{ArtifactReader, ZipArchiveReader};

use crate::VsError;

/// What a VaultSpeed zip says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ZipInfo {
    /// `1182_FMC`
    pub name: String,
    pub number: u64,
    /// `FMC`, `DDL`, `GENERATE_ETL`, `COMP`
    pub kind: String,
    /// Jira tickets named in VaultSpeed's comment headers (`Comment: INTEL-21250 ...`).
    pub tickets: Vec<String>,
    /// VaultSpeed's own release (`Release_181a(181)`).
    pub vs_release: Option<String>,
    pub files: usize,
}

/// `1182_FMC.zip` (or `1182_FMC`) → (`1182_FMC`, 1182, `FMC`).
pub fn parse_zip_name(file: &str) -> Option<(String, u64, String)> {
    let name = file.rsplit('/').next()?.trim_end_matches(".zip");
    let (number, kind) = name.split_once('_')?;
    let number: u64 = number.parse().ok()?;
    let valid_kind = !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    valid_kind.then(|| (name.to_owned(), number, kind.to_owned()))
}

/// Reads the zip in memory and collects the tickets from the comment headers.
pub fn inspect(file: &str, bytes: Vec<u8>, ticket_prefix: &str) -> Result<ZipInfo, VsError> {
    let (name, number, kind) = parse_zip_name(file).ok_or_else(|| VsError::Zip {
        name: file.to_owned(),
        detail: "not a <number>_<TYPE>.zip name".into(),
    })?;
    let files = ZipArchiveReader::from_bytes(PathBuf::from(file), bytes)
        .read()
        .map_err(|e| VsError::Zip {
            name: name.clone(),
            detail: e.to_string(),
        })?;
    let ticket = Regex::new(&format!(r"(?i)\b{}-\d+\b", regex::escape(ticket_prefix)))
        .expect("valid pattern");
    let release = Regex::new(r"Release: (Release_[^\s,]+)").expect("valid pattern");
    let mut tickets = BTreeSet::new();
    let mut vs_release = None;
    for file in &files {
        for line in file.content.lines().filter(|l| l.contains("Comment:")) {
            // Only the comment part: `... - Comment: INTEL-17103 Add Talenkennis - Release date: ...`
            let comment = line
                .split("Comment:")
                .skip(1)
                .map(|c| c.split(" - Release date").next().unwrap_or(c))
                .collect::<Vec<_>>()
                .join(" ");
            tickets.extend(
                ticket
                    .find_iter(&comment)
                    .map(|m| m.as_str().to_ascii_uppercase()),
            );
            if vs_release.is_none() && line.contains("DV_NAME") {
                vs_release = release.captures(line).map(|c| c[1].to_owned());
            }
        }
    }
    Ok(ZipInfo {
        name,
        number,
        kind,
        tickets: tickets.into_iter().collect(),
        vs_release,
        files: files.len(),
    })
}

/// Inspections by blob id: a blob never changes, so neither does what it says.
#[derive(Debug, Default)]
pub struct ZipCache {
    by_blob: Mutex<HashMap<String, ZipInfo>>,
}

impl ZipCache {
    /// The zip at `rev:path`, inspected once per blob.
    pub fn get(
        &self,
        vcs: &dyn Vcs,
        rev: &str,
        path: &str,
        ticket_prefix: &str,
    ) -> Result<ZipInfo, VsError> {
        let blob = vcs.blob_id(rev, path)?.ok_or_else(|| VsError::Zip {
            name: path.to_owned(),
            detail: format!("not in {rev}"),
        })?;
        if let Some(info) = self.lock().get(&blob) {
            return Ok(info.clone());
        }
        let bytes = vcs.show_bytes(rev, path)?.ok_or_else(|| VsError::Zip {
            name: path.to_owned(),
            detail: format!("not in {rev}"),
        })?;
        let info = inspect(path, bytes, ticket_prefix)?;
        self.lock().insert(blob, info.clone());
        Ok(info)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, ZipInfo>> {
        self.by_blob
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    pub(crate) fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            for (name, content) in files {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(content.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    #[test]
    fn tickets_come_from_the_comment_headers_only() {
        let zip = zip_with(&[
            (
                "1182_FL_DAG_x.py",
                "# DV_NAME: edw - Release: Release_181a(181) - Comment: INTEL-21250 - Release date: 2025/12/02\n\
                 # SRC_NAME: x - Release: x(18) - Comment: intel-21251 fix - Release date: 2025/12/02\n\
                 sql = 'INTEL-99999 in code is not a ticket'\n",
            ),
            ("meta.json", "{}"),
        ]);
        let info = inspect("vaultspeed/staging/release/1182_FMC.zip", zip, "INTEL").unwrap();
        assert_eq!(info.name, "1182_FMC");
        assert_eq!(info.number, 1182);
        assert_eq!(info.kind, "FMC");
        assert_eq!(info.tickets, ["INTEL-21250", "INTEL-21251"]);
        assert_eq!(info.vs_release.as_deref(), Some("Release_181a(181)"));
        assert_eq!(info.files, 2);
    }

    #[test]
    fn zip_names() {
        assert_eq!(
            parse_zip_name("1178_GENERATE_ETL.zip"),
            Some(("1178_GENERATE_ETL".into(), 1178, "GENERATE_ETL".into()))
        );
        assert_eq!(parse_zip_name("readme.txt"), None);
        assert_eq!(parse_zip_name("x_FMC.zip"), None);
    }
}
