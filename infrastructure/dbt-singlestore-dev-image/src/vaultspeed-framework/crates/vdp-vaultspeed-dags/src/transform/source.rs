/// Determines the source system (`ami`, `refkader`, ...) a DAG belongs to.
#[derive(Debug, Clone, Default)]
pub struct SourceResolver {
    known: Vec<String>,
}

impl SourceResolver {
    /// Creates a resolver that knows the source systems `known` (compared in lower case).
    pub fn new(known: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            known: known.into_iter().map(|s| s.into().to_lowercase()).collect(),
        }
    }

    /// A configured source that appears as a token of the file name wins; otherwise the
    /// `VaultSpeed` naming convention `<gen>_FL_DAG_<source>_<load>_...` is used.
    pub fn resolve(&self, file_name: &str) -> Option<String> {
        let stem = file_name
            .rsplit_once('.')
            .map_or(file_name, |(stem, _)| stem);
        let tokens: Vec<String> = stem.split('_').map(str::to_lowercase).collect();
        if let Some(known) = tokens.iter().find(|t| self.known.contains(t)) {
            return Some(known.clone());
        }
        tokens
            .windows(2)
            .find(|w| w[0] == "dag")
            .and_then(|_| tokens.iter().skip_while(|t| *t != "dag").nth(1).cloned())
            .filter(|t| !t.is_empty() && !t.bytes().all(|b| b.is_ascii_digit()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_known_sources_then_naming_convention() {
        let resolver = SourceResolver::new(["AMI", "REFKADER"]);
        assert_eq!(
            resolver
                .resolve("1134_FL_DAG_refkader_incr_20251023.py")
                .as_deref(),
            Some("refkader")
        );
        assert_eq!(
            resolver
                .resolve("700_FL_DAG_ksz_init_20250101.py")
                .as_deref(),
            Some("ksz")
        );
        assert_eq!(resolver.resolve("notes.py"), None);
    }
}
