use std::fmt::Debug;
use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;

use vdp_vaultspeed_domain::RelPath;
use walkdir::WalkDir;

use crate::sources::{SourceError, SourceFile, SourceLocation};

/// Reads every file of an artifact into memory, sorted by path.
pub trait ArtifactReader: Send + Sync + Debug {
    fn read(&self) -> Result<Vec<SourceFile>, SourceError>;
}

/// Picks the reader for a location.
pub fn open_reader(location: &SourceLocation) -> Box<dyn ArtifactReader> {
    match location {
        SourceLocation::Zip(path) => Box::new(ZipArchiveReader::new(path.clone())),
        SourceLocation::Directory(path) => Box::new(DirectoryReader::new(path.clone())),
    }
}

/// Reads a generation zip without extracting it to disk.
#[derive(Debug, Clone)]
pub struct ZipArchiveReader {
    path: PathBuf,
    /// The archive itself when it is not a file (a git blob); `path` then only labels it.
    bytes: Option<std::sync::Arc<Vec<u8>>>,
    max_total_bytes: u64,
}

impl ZipArchiveReader {
    /// Generations are a few MB; anything beyond this is treated as a zip bomb.
    pub const DEFAULT_LIMIT: u64 = 1 << 30;

    pub const fn new(path: PathBuf) -> Self {
        Self {
            path,
            bytes: None,
            max_total_bytes: Self::DEFAULT_LIMIT,
        }
    }

    /// An archive in memory (e.g. read from git); `label` names it in errors.
    pub fn from_bytes(label: PathBuf, bytes: Vec<u8>) -> Self {
        Self {
            path: label,
            bytes: Some(std::sync::Arc::new(bytes)),
            max_total_bytes: Self::DEFAULT_LIMIT,
        }
    }

    #[must_use]
    pub const fn with_limit(mut self, max_total_bytes: u64) -> Self {
        self.max_total_bytes = max_total_bytes;
        self
    }
}

impl ArtifactReader for ZipArchiveReader {
    fn read(&self) -> Result<Vec<SourceFile>, SourceError> {
        match &self.bytes {
            Some(bytes) => self.read_from(std::io::Cursor::new(bytes.as_slice())),
            None => self.read_from(File::open(&self.path).map_err(SourceError::io(&self.path))?),
        }
    }
}

impl ZipArchiveReader {
    fn read_from<R: std::io::Read + std::io::Seek>(
        &self,
        reader: R,
    ) -> Result<Vec<SourceFile>, SourceError> {
        let zip_error = |source| SourceError::Zip {
            path: self.path.clone(),
            source,
        };
        let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;
        let mut files = Vec::new();
        let mut total: u64 = 0;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(zip_error)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry
                .enclosed_name()
                .ok_or_else(|| SourceError::UnsafeEntry {
                    archive: self.path.clone(),
                    entry: entry
                        .name()
                        .map_or_else(|e| e.to_string(), std::borrow::Cow::into_owned),
                })?;
            total = total.saturating_add(entry.size());
            if total > self.max_total_bytes {
                return Err(SourceError::TooLarge {
                    archive: self.path.clone(),
                    limit: self.max_total_bytes,
                });
            }
            let path = RelPath::new(name.to_string_lossy());
            let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or_default());
            entry
                .read_to_end(&mut bytes)
                .map_err(SourceError::io(&self.path))?;
            files.push(SourceFile::decode(path, &bytes));
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }
}

/// Reads all files below a directory.
#[derive(Debug, Clone)]
pub struct DirectoryReader {
    root: PathBuf,
}

impl DirectoryReader {
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl ArtifactReader for DirectoryReader {
    fn read(&self) -> Result<Vec<SourceFile>, SourceError> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for entry in WalkDir::new(&self.root).sort_by_file_name() {
            let entry = entry.map_err(|e| SourceError::Io {
                path: self.root.clone(),
                source: e.into(),
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(entry.path());
            let bytes = fs::read(entry.path()).map_err(SourceError::io(entry.path()))?;
            files.push(SourceFile::decode(
                RelPath::new(rel.to_string_lossy()),
                &bytes,
            ));
        }
        Ok(files)
    }
}
