use std::path::Path;

/// How a rendered file compares to what is currently on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Created,
    Changed,
    Unchanged,
}

impl FileStatus {
    /// Compares what is on disk at `path` with `rendered`: [`FileStatus::Unchanged`] when equal, [`FileStatus::Changed`]
    /// when different and [`FileStatus::Created`] when the file does not exist.
    ///
    /// # Errors
    ///
    /// The [`std::io::Error`] from reading `path`, unless the file is missing (that is `Created`): permission denied, a
    /// directory instead of a file, or content that is not UTF-8.
    pub fn diff(path: &Path, rendered: &str) -> std::io::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(existing) if existing == rendered => Ok(Self::Unchanged),
            Ok(_) => Ok(Self::Changed),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::Created),
            Err(e) => Err(e),
        }
    }

    /// The lower-case word for the status (`created`, `changed`, `unchanged`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Changed => "changed",
            Self::Unchanged => "unchanged",
        }
    }
}
