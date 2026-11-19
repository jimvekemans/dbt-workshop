use std::collections::BTreeMap;
use std::fmt;

use secrecy::SecretString;

use crate::SecretProvider;

/// Fixed values, e.g. secrets computed by the toolkit itself or test fixtures.
pub struct MapProvider {
    label: String,
    values: BTreeMap<String, SecretString>,
}

impl MapProvider {
    /// Creates an empty provider that reports itself as `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            values: BTreeMap::new(),
        }
    }

    /// Returns the provider with `key` set to `value`.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.insert(key, value);
        self
    }

    /// Sets `key` to `value`, replacing an earlier value.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.values
            .insert(key.into(), SecretString::from(value.into()));
    }
}

impl fmt::Debug for MapProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MapProvider")
            .field("label", &self.label)
            .field("keys", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl SecretProvider for MapProvider {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        self.values.get(key).cloned()
    }

    fn keys(&self) -> Vec<String> {
        self.values.keys().cloned().collect()
    }
}
