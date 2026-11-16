use secrecy::SecretString;

use crate::SecretProvider;

/// Reads the process environment (CI variables).
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvProvider;

impl SecretProvider for EnvProvider {
    fn name(&self) -> String {
        "process environment".into()
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        std::env::var(key)
            .ok()
            .filter(|v| !v.is_empty())
            .map(SecretString::from)
    }
}
