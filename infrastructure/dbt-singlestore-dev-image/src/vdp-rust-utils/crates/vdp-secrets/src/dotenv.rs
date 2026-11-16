use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use secrecy::SecretString;

use crate::{SecretError, SecretProvider};

/// Values from a `.env` file, parsed without modifying the process environment.
///
/// The file is the project's `.devcontainer/.env`, as in the Python utilities.
pub struct DotenvProvider {
    path: PathBuf,
    values: HashMap<String, SecretString>,
}

impl DotenvProvider {
    /// Loads the file; a missing file yields an empty provider.
    pub fn load(path: &Path) -> Result<Self, SecretError> {
        let mut values = HashMap::new();
        if path.exists() {
            let iter = dotenvy::from_path_iter(path).map_err(|e| SecretError::File {
                path: path.to_owned(),
                detail: e.to_string(),
            })?;
            for item in iter {
                let (key, value) = item.map_err(|e| SecretError::File {
                    path: path.to_owned(),
                    detail: e.to_string(),
                })?;
                values.insert(key, SecretString::from(value));
            }
        }
        Ok(Self {
            path: path.to_owned(),
            values,
        })
    }

    /// Whether the file existed and held at least one value.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl fmt::Debug for DotenvProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DotenvProvider")
            .field("path", &self.path)
            .field("keys", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl SecretProvider for DotenvProvider {
    fn name(&self) -> String {
        self.path.display().to_string()
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        self.values.get(key).cloned()
    }

    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.values.keys().cloned().collect();
        keys.sort();
        keys
    }
}
