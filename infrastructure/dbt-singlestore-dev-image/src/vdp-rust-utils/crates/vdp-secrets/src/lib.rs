//! Secret resolution without touching the process environment.
//!
//! A building block. The Python utilities exported every secret into `os.environ`, leaking them into every subprocess and
//! log. Here providers are queried explicitly and values stay wrapped in [`SecretString`] (redacted in `Debug`) until the
//! one place that needs them: a database connection, or the environment of the one child process (dbt) that renders them
//! through `env_var()`. Shared by every module, VaultSpeed deployments included.
//!
//! # Responsibilities
//!
//! - Define the [`SecretProvider`] port (`get`, plus [`SecretProvider::keys`] and [`SecretProvider::layers`]).
//! - Provide the sources: the process environment ([`EnvProvider`]), a dotenv file ([`DotenvProvider`]), Vault KV v2
//!   ([`VaultClient`], [`VaultKvProvider`]) and a plain map ([`MapProvider`], for tests and fixed values).
//! - Combine them by precedence ([`ChainProvider`]).
//! - Look keys up ([`first_of`], [`require`]) and hand a child process exactly its environment ([`child_env`]).
//!
//! # Example
//!
//! ```
//! use vdp_secrets::{ChainProvider, ExposeSecret, MapProvider, SecretProvider, require};
//!
//! // The first provider that has a key wins.
//! let secrets = ChainProvider::new(vec![
//!     Box::new(MapProvider::new("local").with("SINGLESTORE_HOST", "localhost")),
//!     Box::new(MapProvider::new("vault").with("SINGLESTORE_HOST", "db.example").with("SINGLESTORE_USER", "dbt")),
//! ]);
//! let host = require(&secrets, &["SINGLESTORE_HOST", "SINGLESTORE_URL"])?;
//! assert_eq!(host.expose_secret(), "localhost");
//! assert_eq!(secrets.source_of("SINGLESTORE_USER").as_deref(), Some("vault"));
//! assert!(require(&secrets, &["MISSING"]).is_err());
//! # Ok::<(), vdp_secrets::SecretError>(())
//! ```

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
pub use vault::{AppRoleCredentials, VaultClient, VaultKvProvider, normalize_url};

#[doc(hidden)]
pub trait AsSecretProvider {
    fn as_secret_provider(&self) -> &dyn SecretProvider;
}

impl<T: SecretProvider> AsSecretProvider for T {
    fn as_secret_provider(&self) -> &dyn SecretProvider {
        self
    }
}

/// A source of secret values.
///
/// Responsibilities of an implementor: say who it is ([`name`](Self::name)), answer lookups
/// ([`get`](Self::get)), and optionally list the keys it holds ([`keys`](Self::keys)).
///
/// Implementors must be [`Sized`]. Every `T: SecretProvider` gets the hidden supertrait `AsSecretProvider` through a
/// blanket impl, and its `&self -> &dyn SecretProvider` conversion needs a sized `Self`; an unsized
/// `impl SecretProvider for dyn X` therefore no longer compiles. Put a trait object behind a sized wrapper instead.
///
/// [`layers`](Self::layers) has a default implementation that returns this provider as the single layer; only
/// composite providers (such as [`ChainProvider`]) override it, to expose their sources in precedence order.
///
/// # Example
///
/// ```
/// use vdp_secrets::{ExposeSecret, SecretProvider, SecretString};
///
/// #[derive(Debug)]
/// struct Fixed;
///
/// impl SecretProvider for Fixed {
///     fn name(&self) -> String {
///         "fixed".into()
///     }
///
///     fn get(&self, key: &str) -> Option<SecretString> {
///         (key == "TOKEN").then(|| SecretString::from("s3cret"))
///     }
/// }
///
/// assert_eq!(Fixed.get("TOKEN").unwrap().expose_secret(), "s3cret");
/// assert!(Fixed.keys().is_empty());          // the default: no keys listed
/// assert_eq!(Fixed.layers().len(), 1);       // the default: a single layer
/// ```
pub trait SecretProvider: Send + Sync + Debug + AsSecretProvider {
    /// A name for this provider, used in log lines and error messages.
    fn name(&self) -> String;
    /// The value for `key`, or `None` when this provider does not hold it.
    fn get(&self, key: &str) -> Option<SecretString>;

    /// The keys this provider holds itself, in a stable order. The process environment returns none: a child
    /// process inherits it anyway.
    fn keys(&self) -> Vec<String> {
        Vec::new()
    }

    /// Individual source maps in precedence order; leaf providers return `[self]`.
    fn layers(&self) -> Vec<&dyn SecretProvider> {
        vec![self.as_secret_provider()]
    }
}

/// Looks up the first available key among alternatives (`AIRFLOW_URL` or `AIRFLOW_HOST`).
pub fn first_of(provider: &dyn SecretProvider, keys: &[&str]) -> Option<SecretString> {
    keys.iter().find_map(|key| provider.get(key))
}

/// Like [`first_of`], failing with a message naming every alternative.
///
/// # Errors
///
/// [`SecretError::Missing`] when `provider` holds none of `keys`.
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
