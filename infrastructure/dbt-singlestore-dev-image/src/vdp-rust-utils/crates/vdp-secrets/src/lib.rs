//! Secret resolution without touching the process environment.
//!
//! The Python utilities exported every secret into `os.environ`, leaking them into every subprocess and log.
//! Here providers are queried explicitly and values stay wrapped in [`SecretString`] (redacted in `Debug`) until
//! the one place that needs them: a database connection, or the environment of the one child process (dbt) that
//! renders them through `env_var()`.
//!
//! Shared by every module, VaultSpeed deployments included (the framework's own copy was merged into this one),
//! with [`SecretProvider::keys`], [`MapProvider`] and [`child_env`].

mod chain;
mod dotenv;
mod env;
mod error;
mod map;
mod vault;

use std::fmt::Debug;

pub use chain::ChainProvider;
pub use dotenv::DotenvProvider;
pub use env::EnvProvider;
pub use error::SecretError;
pub use map::MapProvider;
pub use secrecy::{ExposeSecret, SecretString};
pub use vault::{VaultClient, VaultKvProvider};

/// A source of secret values.
pub trait SecretProvider: Send + Sync + Debug {
    fn name(&self) -> String;
    fn get(&self, key: &str) -> Option<SecretString>;

    /// The keys this provider holds itself, in a stable order. The process environment returns none: a child
    /// process inherits it anyway.
    fn keys(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Looks up the first available key among alternatives (`AIRFLOW_URL` or `AIRFLOW_HOST`).
pub fn first_of(provider: &dyn SecretProvider, keys: &[&str]) -> Option<SecretString> {
    keys.iter().find_map(|key| provider.get(key))
}

/// Like [`first_of`], failing with a message naming every alternative.
pub fn require(provider: &dyn SecretProvider, keys: &[&str]) -> Result<SecretString, SecretError> {
    first_of(provider, keys).ok_or_else(|| SecretError::Missing {
        keys: keys.join(" or "),
        provider: provider.name(),
    })
}

/// Every key the provider holds, with the value that wins, for the environment of a child process.
pub fn child_env(provider: &dyn SecretProvider) -> Vec<(String, SecretString)> {
    provider
        .keys()
        .into_iter()
        .filter_map(|key| provider.get(&key).map(|value| (key, value)))
        .collect()
}
