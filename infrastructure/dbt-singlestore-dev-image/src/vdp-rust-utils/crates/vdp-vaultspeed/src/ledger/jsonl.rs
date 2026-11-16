use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry};

use crate::ledger::{LedgerError, LedgerReader, LedgerSink, LedgerWriter, MemoryLedger};

/// Append-only JSON-lines ledger, one entry per line. Diff-friendly in git.
#[derive(Debug)]
pub struct JsonlLedger {
    path: PathBuf,
    memory: MemoryLedger,
    file: Mutex<File>,
}

impl JsonlLedger {
    /// Opens (creating if needed) the ledger file and loads all entries.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, LedgerError> {
        let path = path.into();
        let io = |source| LedgerError::Io {
            path: path.clone(),
            source,
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io)?;
        }
        let entries = if path.exists() {
            read_entries(&path)?
        } else {
            Vec::new()
        };
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io)?;
        Ok(Self {
            memory: MemoryLedger::from_entries(entries),
            file: Mutex::new(file),
            path,
        })
    }

    /// Reads the file into memory without creating or locking anything (preview runs).
    pub fn snapshot(path: &Path) -> Result<MemoryLedger, LedgerError> {
        let entries = if path.exists() {
            read_entries(path)?
        } else {
            Vec::new()
        };
        Ok(MemoryLedger::from_entries(entries))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_line(&self, entry: &LedgerEntry) -> Result<(), LedgerError> {
        let mut line = serde_json::to_string(entry).map_err(|source| LedgerError::Corrupt {
            path: self.path.clone(),
            line: 0,
            source,
        })?;
        line.push('\n');
        let mut file = self.file.lock().map_err(|_| LedgerError::Poisoned)?;
        file.write_all(line.as_bytes())
            .and_then(|()| file.flush())
            .map_err(|source| LedgerError::Io {
                path: self.path.clone(),
                source,
            })
    }
}

fn read_entries(path: &Path) -> Result<Vec<LedgerEntry>, LedgerError> {
    let file = File::open(path).map_err(|source| LedgerError::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut entries = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| LedgerError::Io {
            path: path.to_owned(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str(&line).map_err(|source| LedgerError::Corrupt {
            path: path.to_owned(),
            line: index + 1,
            source,
        })?;
        entries.push(entry);
    }
    Ok(entries)
}

impl LedgerReader for JsonlLedger {
    fn find_applied(
        &self,
        raw: Option<&Fingerprint>,
        effective: &Fingerprint,
    ) -> Result<Option<LedgerEntry>, LedgerError> {
        self.memory.find_applied(raw, effective)
    }

    fn latest_for_object(&self, object_key: &str) -> Result<Option<LedgerEntry>, LedgerError> {
        self.memory.latest_for_object(object_key)
    }

    fn last_sequence(&self) -> Result<u64, LedgerError> {
        self.memory.last_sequence()
    }

    fn entries(&self) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.memory.entries()
    }
}

impl LedgerWriter for JsonlLedger {
    fn record(&self, entry: LedgerEntry) -> Result<LedgerEntry, LedgerError> {
        let recorded = self.memory.record(entry)?;
        self.write_line(&recorded)?;
        Ok(recorded)
    }
}

impl LedgerSink for JsonlLedger {
    fn append(&self, entry: &LedgerEntry) -> Result<(), LedgerError> {
        self.memory.append(entry)?;
        self.write_line(entry)
    }
}

#[cfg(test)]
mod tests {
    use vdp_vaultspeed_domain::LedgerOutcome;

    use super::*;
    use crate::ledger::memory::tests::entry;

    #[test]
    fn survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger/LDV.jsonl");
        {
            let ledger = JsonlLedger::open(&path).unwrap();
            ledger
                .record(entry(Some("a"), "a", LedgerOutcome::Applied))
                .unwrap();
            ledger
                .record(entry(Some("b"), "b", LedgerOutcome::Failed))
                .unwrap();
        }
        let reopened = JsonlLedger::open(&path).unwrap();
        assert_eq!(reopened.entries().unwrap().len(), 2);
        assert_eq!(reopened.last_sequence().unwrap(), 2);
        assert!(
            reopened
                .find_applied(Some(&Fingerprint::of("a")), &Fingerprint::of("a"))
                .unwrap()
                .is_some()
        );
        assert!(
            reopened
                .find_applied(Some(&Fingerprint::of("b")), &Fingerprint::of("b"))
                .unwrap()
                .is_none()
        );
        let next = reopened
            .record(entry(Some("c"), "c", LedgerOutcome::Applied))
            .unwrap();
        assert_eq!(next.sequence, 3);
    }

    #[test]
    fn snapshots_never_create_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger/LDV.jsonl");
        assert_eq!(
            JsonlLedger::snapshot(&path)
                .unwrap()
                .last_sequence()
                .unwrap(),
            0
        );
        assert!(!path.exists());
    }

    #[test]
    fn reports_corrupt_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.jsonl");
        fs::write(&path, "{not json}\n").unwrap();
        assert!(matches!(
            JsonlLedger::open(&path),
            Err(LedgerError::Corrupt { line: 1, .. })
        ));
    }
}
