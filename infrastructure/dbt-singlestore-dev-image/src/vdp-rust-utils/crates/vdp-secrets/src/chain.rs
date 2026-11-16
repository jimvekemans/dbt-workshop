use std::collections::BTreeSet;

use secrecy::SecretString;

use crate::SecretProvider;

/// Queries providers in order; the first one that knows a key wins.
#[derive(Debug, Default)]
pub struct ChainProvider {
    providers: Vec<Box<dyn SecretProvider>>,
}

impl ChainProvider {
    pub fn new(providers: Vec<Box<dyn SecretProvider>>) -> Self {
        Self { providers }
    }

    #[must_use]
    pub fn with(mut self, provider: Box<dyn SecretProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    pub fn push(&mut self, provider: Box<dyn SecretProvider>) {
        self.providers.push(provider);
    }

    /// The providers' names, highest priority first.
    pub fn provider_names(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    /// Which provider a key resolves from.
    pub fn source_of(&self, key: &str) -> Option<String> {
        self.providers
            .iter()
            .find(|p| p.get(key).is_some())
            .map(|p| p.name())
    }
}

impl SecretProvider for ChainProvider {
    fn name(&self) -> String {
        self.provider_names().join(", ")
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        self.providers.iter().find_map(|p| p.get(key))
    }

    fn keys(&self) -> Vec<String> {
        let mut keys = BTreeSet::new();
        for provider in &self.providers {
            keys.extend(provider.keys());
        }
        keys.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
    use crate::MapProvider;

    #[test]
    fn first_provider_wins_and_missing_keys_name_all_providers() {
        let chain = ChainProvider::new(vec![
            Box::new(MapProvider::new("ci").with("A", "from-ci")),
            Box::new(
                MapProvider::new("vault")
                    .with("A", "from-vault")
                    .with("B", "b"),
            ),
        ]);
        assert_eq!(chain.get("A").unwrap().expose_secret(), "from-ci");
        assert_eq!(chain.get("B").unwrap().expose_secret(), "b");
        assert_eq!(chain.source_of("B").as_deref(), Some("vault"));
        let error = crate::require(&chain, &["C", "D"]).unwrap_err().to_string();
        assert_eq!(error, "secret C or D not found in ci, vault");
    }

    #[test]
    fn child_env_has_every_key_once_with_the_winning_value() {
        let chain = ChainProvider::new(vec![
            Box::new(MapProvider::new("high").with("A", "1")),
            Box::new(MapProvider::new("low").with("A", "0").with("B", "2")),
        ]);
        let env: Vec<(String, String)> = crate::child_env(&chain)
            .into_iter()
            .map(|(k, v)| (k, v.expose_secret().to_owned()))
            .collect();
        assert_eq!(env, [("A".into(), "1".into()), ("B".into(), "2".into())]);
    }
}
