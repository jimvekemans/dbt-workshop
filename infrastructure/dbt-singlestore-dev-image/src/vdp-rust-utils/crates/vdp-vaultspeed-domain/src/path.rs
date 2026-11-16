use std::fmt;

use serde::{Deserialize, Serialize};

/// A `/`-separated path relative to the root of an artifact (zip or directory).
///
/// Using one canonical separator keeps job identities stable between Windows
/// developer machines, Linux CI runners and zip archives.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelPath(String);

impl RelPath {
    pub fn new(path: impl AsRef<str>) -> Self {
        let normalised = path
            .as_ref()
            .split(['/', '\\'])
            .filter(|part| !part.is_empty() && *part != ".")
            .collect::<Vec<_>>()
            .join("/");
        Self(normalised)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }

    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or_default()
    }

    /// File name without its last extension.
    pub fn file_stem(&self) -> &str {
        let name = self.file_name();
        name.rsplit_once('.').map_or(name, |(stem, _)| stem)
    }

    pub fn extension(&self) -> Option<&str> {
        self.file_name().rsplit_once('.').map(|(_, ext)| ext)
    }

    pub fn has_extension(&self, ext: &str) -> bool {
        self.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    }

    /// The directory part, empty for top-level files.
    #[must_use]
    pub fn parent(&self) -> Self {
        Self(
            self.0
                .rsplit_once('/')
                .map(|(dir, _)| dir.to_owned())
                .unwrap_or_default(),
        )
    }

    /// Strips a leading directory, returning `None` when the path is not below it.
    pub fn strip_prefix(&self, dir: &str) -> Option<Self> {
        let dir = Self::new(dir);
        if dir.0.is_empty() {
            return Some(self.clone());
        }
        self.0
            .strip_prefix(&dir.0)
            .and_then(|rest| rest.strip_prefix('/'))
            .map(Self::new)
    }

    #[must_use]
    pub fn join(&self, child: &str) -> Self {
        Self::new(format!("{}/{child}", self.0))
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_separators() {
        let path = RelPath::new("\\ETL\\INCR//FOUNDATION_LAYER/x.sql");
        assert_eq!(path.as_str(), "ETL/INCR/FOUNDATION_LAYER/x.sql");
        assert_eq!(path.file_stem(), "x");
        assert_eq!(path.parent().as_str(), "ETL/INCR/FOUNDATION_LAYER");
    }

    #[test]
    fn strips_prefixes_on_component_boundaries() {
        let path = RelPath::new("ETL/INCR/a.sql");
        assert_eq!(path.strip_prefix("ETL").unwrap().as_str(), "INCR/a.sql");
        assert!(path.strip_prefix("ET").is_none());
        assert!(path.strip_prefix("DDL").is_none());
    }
}
