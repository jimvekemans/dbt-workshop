use vdp_vaultspeed_domain::RelPath;

/// A file read from an artifact, with its text decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: RelPath,
    pub content: String,
}

impl SourceFile {
    pub fn new(path: RelPath, content: impl Into<String>) -> Self {
        Self {
            path,
            content: content.into(),
        }
    }

    /// Decodes bytes as UTF-8 (dropping a BOM) and falls back to Latin-1, which
    /// `VaultSpeed` uses for some source metadata exports. Decoding never fails.
    pub fn decode(path: RelPath, bytes: &[u8]) -> Self {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let content = if let Ok(text) = std::str::from_utf8(bytes) {
            text.to_owned()
        } else {
            tracing::warn!(%path, "file is not UTF-8; decoding as Latin-1");
            bytes.iter().map(|&b| char::from(b)).collect()
        };
        Self { path, content }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_with_bom_and_latin1() {
        assert_eq!(
            SourceFile::decode(RelPath::new("a"), b"\xEF\xBB\xBFselect").content,
            "select"
        );
        assert_eq!(
            SourceFile::decode(RelPath::new("a"), b"caf\xE9").content,
            "café"
        );
    }
}
