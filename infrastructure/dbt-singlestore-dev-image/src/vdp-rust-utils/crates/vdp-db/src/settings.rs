use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use vdp_secrets::SecretProvider;

/// TLS behaviour, using the `ssl_mode` vocabulary of the dbt v2 SingleStore profile
/// (`/opt/dbt/crates/dbt-auth/src/singlestore/mod.rs`), so a profile means the same thing to dbt and to us.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// Plain TCP (`disabled`).
    #[default]
    Disabled,
    /// TLS when the server offers it, without certificate checks; plain TCP otherwise (`preferred`).
    Preferred,
    /// TLS required, certificate not verified (`required`).
    SkipVerify,
    /// TLS required, certificate chain verified, host name not checked (`verify_ca`).
    VerifyCa,
    /// TLS required, chain and host name verified (`verify_full`).
    VerifyFull,
}

impl TlsMode {
    /// Parses a profile `ssl_mode` value with the aliases dbt accepts.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "disable" | "disabled" | "false" | "off" => Self::Disabled,
            "prefer" | "preferred" => Self::Preferred,
            "require" | "required" | "true" => Self::SkipVerify,
            "verify_ca" | "verify-ca" | "verifyca" => Self::VerifyCa,
            "verify_full" | "verify-full" | "verifyfull" | "verifyidentity" | "verify-identity" => {
                Self::VerifyFull
            }
            _ => return None,
        })
    }

    /// Whether the mode makes the connection use TLS (everything but `Disabled` and `Preferred`).
    pub const fn is_enforced(&self) -> bool {
        matches!(self, Self::SkipVerify | Self::VerifyCa | Self::VerifyFull)
    }
}

/// A client certificate and its private key (PEM files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCertificate {
    pub certificate: PathBuf,
    pub key: PathBuf,
}

/// Errors when building [`ConnectionSettings`] from a map or [`SecretProvider`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("missing required connection setting: {keys}")]
    MissingRequired { field: &'static str, keys: String },
    #[error("conflicting synonyms for {field}: '{first_key}' and '{second_key}'")]
    ConflictingSynonyms {
        field: &'static str,
        first_key: String,
        second_key: String,
    },
    #[error("invalid port in '{key}': '{value}'")]
    InvalidPort { key: String, value: String },
}

/// Everything needed to open a SingleStore session.
#[derive(Clone)]
pub struct ConnectionSettings {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<SecretString>,
    pub database: Option<String>,
    pub tls: TlsMode,
    /// PEM file of the CA that signed the server certificate.
    pub ca_certificate: Option<PathBuf>,
    pub client_certificate: Option<ClientCertificate>,
    pub connect_timeout: Duration,
    /// Non-synonym keys preserved by [`ConnectionSettings::from_map`] and [`ConnectionSettings::from_secrets`].
    pub extra: BTreeMap<String, String>,
}

const DEFAULT_PORT: u16 = 3306;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

const PROBED_SYNONYMS: [&str; 12] = [
    "HOST", "SERVER", "URL", "USER", "USERNAME", "PASS", "PASSWORD", "PWD", "DBNAME", "DB",
    "DATABASE", "PORT",
];

impl ConnectionSettings {
    /// Creates settings for `user` on `host:port` without password or database, TLS off and the default connect timeout.
    pub fn new(host: impl Into<String>, port: u16, user: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            user: user.into(),
            password: None,
            database: None,
            tls: TlsMode::Disabled,
            ca_certificate: None,
            client_certificate: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            extra: BTreeMap::new(),
        }
    }

    /// Parses connection settings from an arbitrary key-value map using case-insensitive synonyms
    /// (`host`/`server`/`url`, `user`/`username`, `pass`/`password`/`pwd`, `dbname`/`db`/`database`,
    /// `port`), preserving unknown keys in [`ConnectionSettings::extra`].
    ///
    /// # Errors
    ///
    /// [`SettingsError::MissingRequired`] when no host, user or password key is present; [`SettingsError::ConflictingSynonyms`]
    /// when two synonyms of one setting are both present; [`SettingsError::InvalidPort`] when the port is not a number
    /// below 65536.
    pub fn from_map<I, K, V>(entries: I) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let normalized = entries
            .into_iter()
            .map(|(k, v)| {
                let key = k.as_ref().to_owned();
                let val = v.as_ref().to_owned();
                (key.clone(), key, val)
            })
            .collect::<Vec<_>>();
        let parsed = parse_single_layer(&normalized, false, false)?;
        finalize_settings(parsed, "", None, None)
    }

    /// Starts building [`ConnectionSettings`] from a [`SecretProvider`], supporting optional key
    /// prefixing (e.g. `"SINGLESTORE_"`), configured port/database overrides, and TLS settings.
    pub const fn from_secrets(secrets: &dyn SecretProvider) -> ConnectionSettingsBuilder<'_> {
        ConnectionSettingsBuilder {
            secrets,
            prefix: String::new(),
            port_override: None,
            database_override: None,
            tls: TlsMode::Disabled,
            ca_certificate: None,
            client_certificate: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }

    /// `user@host:port/database`, without the password.
    pub fn endpoint(&self) -> String {
        format!(
            "{}@{}:{}/{}",
            self.user,
            self.host,
            self.port,
            self.database.as_deref().unwrap_or("")
        )
    }
}

/// Builder returned by [`ConnectionSettings::from_secrets`].
#[derive(Debug)]
pub struct ConnectionSettingsBuilder<'a> {
    secrets: &'a dyn SecretProvider,
    prefix: String,
    port_override: Option<u16>,
    database_override: Option<String>,
    tls: TlsMode,
    ca_certificate: Option<PathBuf>,
    client_certificate: Option<ClientCertificate>,
    connect_timeout: Duration,
}

impl ConnectionSettingsBuilder<'_> {
    /// Reads the keys that start with `prefix` (case-insensitive), so `SOURCE_` finds `SOURCE_HOST`.
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Fixes the connection port to `port`, ignoring any `PORT` secret.
    #[must_use]
    pub const fn port(mut self, port: u16) -> Self {
        self.port_override = Some(port);
        self
    }

    /// Fixes the connection database to `database`, ignoring any `DBNAME`/`DB`/`DATABASE` secret.
    #[must_use]
    pub fn database(mut self, database: impl Into<String>) -> Self {
        self.database_override = Some(database.into());
        self
    }

    /// Sets the TLS mode (default `Disabled`).
    #[must_use]
    pub const fn tls(mut self, tls: TlsMode) -> Self {
        self.tls = tls;
        self
    }

    /// Sets the CA certificate (PEM) that signed the server certificate.
    #[must_use]
    pub fn ca_certificate(mut self, path: impl Into<PathBuf>) -> Self {
        self.ca_certificate = Some(path.into());
        self
    }

    /// Sets the client certificate for mutual TLS.
    #[must_use]
    pub fn client_certificate(mut self, cert: ClientCertificate) -> Self {
        self.client_certificate = Some(cert);
        self
    }

    /// Sets the connect timeout.
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Reads the settings from the secret layers, the highest layer winning for each setting.
    ///
    /// # Errors
    ///
    /// [`SettingsError::MissingRequired`] when no layer holds a host, user or password; [`SettingsError::ConflictingSynonyms`]
    /// when one layer holds two synonyms of one setting; [`SettingsError::InvalidPort`] when the port is not a valid number.
    pub fn build(self) -> Result<ConnectionSettings, SettingsError> {
        let ignore_db = self.database_override.is_some();
        let ignore_port = self.port_override.is_some();
        let mut layer_entries = Vec::new();
        let mut combined_extra = BTreeMap::new();

        for layer in self.secrets.layers() {
            let entries = collect_layer_entries(layer, &self.prefix);
            let parsed = parse_single_layer(&entries, ignore_db, ignore_port)?;
            for (k, v) in parsed.extra {
                combined_extra.entry(k).or_insert(v);
            }
            layer_entries.push(entries);
        }

        let merged = ParsedSlots {
            host: resolve_synonym_across_layers(&layer_entries, &["host", "server", "url"]),
            user: resolve_synonym_across_layers(&layer_entries, &["user", "username"]),
            password: resolve_synonym_across_layers(&layer_entries, &["pass", "password", "pwd"]),
            database: if ignore_db {
                None
            } else {
                resolve_synonym_across_layers(&layer_entries, &["dbname", "db", "database"])
            },
            port: if ignore_port {
                None
            } else {
                resolve_synonym_across_layers(&layer_entries, &["port"])
            },
            extra: combined_extra,
        };

        let mut settings = finalize_settings(
            merged,
            &self.prefix,
            self.port_override,
            self.database_override,
        )?;
        settings.tls = self.tls;
        settings.ca_certificate = self.ca_certificate;
        settings.client_certificate = self.client_certificate;
        settings.connect_timeout = self.connect_timeout;
        Ok(settings)
    }
}

#[derive(Default)]
struct ParsedSlots {
    host: Option<(String, String)>,
    user: Option<(String, String)>,
    password: Option<(String, String)>,
    database: Option<(String, String)>,
    port: Option<(String, String)>,
    extra: BTreeMap<String, String>,
}

fn collect_layer_entries(
    layer: &dyn SecretProvider,
    prefix: &str,
) -> Vec<(String, String, String)> {
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for key in layer.keys() {
        let matches_prefix = prefix.is_empty()
            || (key.len() > prefix.len() && key[..prefix.len()].eq_ignore_ascii_case(prefix));
        if !matches_prefix {
            continue;
        }
        let stripped = key[prefix.len()..].to_owned();
        seen.insert(key.clone());
        if let Some(value) = layer.get(&key) {
            entries.push((key, stripped, value.expose_secret().to_owned()));
        }
    }
    // Providers such as EnvProvider intentionally return no keys from `keys()`, so probe canonical synonyms.
    for syn in PROBED_SYNONYMS {
        let candidates: Vec<String> = if prefix.is_empty() {
            vec![syn.to_owned(), syn.to_ascii_lowercase()]
        } else {
            vec![format!("{prefix}{syn}")]
        };
        for candidate in candidates {
            if seen.contains(&candidate) {
                continue;
            }
            if let Some(value) = layer.get(&candidate) {
                seen.insert(candidate.clone());
                entries.push((candidate, syn.to_owned(), value.expose_secret().to_owned()));
            }
        }
    }
    entries
}

fn parse_single_layer(
    entries: &[(String, String, String)],
    ignore_database: bool,
    ignore_port: bool,
) -> Result<ParsedSlots, SettingsError> {
    let mut slots = ParsedSlots::default();
    for (display_key, lookup_key, value) in entries {
        match lookup_key.trim().to_ascii_lowercase().as_str() {
            "host" | "server" | "url" => {
                assign_synonym(&mut slots.host, "host", display_key.clone(), value.clone())?;
            }
            "user" | "username" => {
                assign_synonym(&mut slots.user, "user", display_key.clone(), value.clone())?;
            }
            "pass" | "password" | "pwd" => assign_synonym(
                &mut slots.password,
                "password",
                display_key.clone(),
                value.clone(),
            )?,
            "dbname" | "db" | "database" if !ignore_database => assign_synonym(
                &mut slots.database,
                "database",
                display_key.clone(),
                value.clone(),
            )?,
            "port" if !ignore_port => {
                assign_synonym(&mut slots.port, "port", display_key.clone(), value.clone())?;
            }
            "dbname" | "db" | "database" | "port" => {}
            _ => {
                slots.extra.insert(lookup_key.clone(), value.clone());
            }
        }
    }
    Ok(slots)
}

fn resolve_synonym_across_layers(
    layers: &[Vec<(String, String, String)>],
    aliases: &[&str],
) -> Option<(String, String)> {
    for alias in aliases {
        for entries in layers {
            if let Some((display_key, _, val)) = entries
                .iter()
                .find(|(_, lookup_key, _)| lookup_key.trim().to_ascii_lowercase() == *alias)
            {
                return Some((display_key.clone(), val.clone()));
            }
        }
    }
    None
}

fn assign_synonym(
    slot: &mut Option<(String, String)>,
    field: &'static str,
    display_key: String,
    value: String,
) -> Result<(), SettingsError> {
    if let Some((prev_key, prev_val)) = slot {
        if *prev_val != value {
            return Err(SettingsError::ConflictingSynonyms {
                field,
                first_key: prev_key.clone(),
                second_key: display_key,
            });
        }
    } else {
        *slot = Some((display_key, value));
    }
    Ok(())
}

fn format_synonym_list(prefix: &str, names: &[&str]) -> String {
    let formatted: Vec<String> = if prefix.is_empty() {
        names.iter().map(|n| (*n).to_owned()).collect()
    } else {
        names
            .iter()
            .map(|n| format!("{prefix}{}", n.to_ascii_uppercase()))
            .collect()
    };
    match formatted.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} or {second}"),
        [init @ .., last] => format!("{}, or {last}", init.join(", ")),
    }
}

fn finalize_settings(
    slots: ParsedSlots,
    prefix: &str,
    port_override: Option<u16>,
    database_override: Option<String>,
) -> Result<ConnectionSettings, SettingsError> {
    let (_, host) = slots.host.ok_or_else(|| SettingsError::MissingRequired {
        field: "host",
        keys: format_synonym_list(prefix, &["host", "server", "url"]),
    })?;
    let (_, user) = slots.user.ok_or_else(|| SettingsError::MissingRequired {
        field: "user",
        keys: format_synonym_list(prefix, &["user", "username"]),
    })?;
    let (_, password) = slots
        .password
        .ok_or_else(|| SettingsError::MissingRequired {
            field: "password",
            keys: format_synonym_list(prefix, &["pass", "password", "pwd"]),
        })?;
    let port = match port_override {
        Some(port) => port,
        None => match slots.port {
            Some((key, raw)) => raw
                .trim()
                .parse::<u16>()
                .map_err(|_| SettingsError::InvalidPort { key, value: raw })?,
            None => DEFAULT_PORT,
        },
    };
    let mut settings = ConnectionSettings::new(host, port, user);
    settings.password = Some(SecretString::from(password));
    settings.database = database_override.or_else(|| slots.database.map(|(_, v)| v));
    settings.extra = slots.extra;
    Ok(settings)
}

impl fmt::Debug for ConnectionSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionSettings")
            .field("endpoint", &self.endpoint())
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("tls", &self.tls)
            .field("ca_certificate", &self.ca_certificate)
            .field("client_certificate", &self.client_certificate)
            .field("connect_timeout", &self.connect_timeout)
            .field("extra_keys", &self.extra.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use vdp_secrets::MapProvider;

    use super::*;

    #[test]
    fn ssl_mode_aliases_match_dbt() {
        assert_eq!(TlsMode::parse("verify-ca"), Some(TlsMode::VerifyCa));
        assert_eq!(TlsMode::parse(" Required "), Some(TlsMode::SkipVerify));
        assert_eq!(TlsMode::parse("verifyidentity"), Some(TlsMode::VerifyFull));
        assert_eq!(TlsMode::parse("off"), Some(TlsMode::Disabled));
        assert_eq!(TlsMode::parse("sometimes"), None);
    }

    #[test]
    fn debug_never_shows_the_password() {
        let mut settings = ConnectionSettings::new("h", 3306, "u");
        settings.password = Some(SecretString::from("hunter2"));
        assert!(!format!("{settings:?}").contains("hunter2"));

        let mapped = ConnectionSettings::from_map([
            ("host", "db.example"),
            ("user", "alice"),
            ("pwd", "s3cr3t-pw"),
            ("api_secret", "never-in-debug"),
        ])
        .unwrap();
        let rendered = format!("{mapped:?}");
        assert!(!rendered.contains("s3cr3t-pw"));
        assert!(!rendered.contains("never-in-debug"));
        assert!(rendered.contains("api_secret"));
    }

    #[test]
    fn from_map_accepts_every_synonym_case_insensitively_and_preserves_extra_keys() {
        let host_variants = [
            ("host", "h1"),
            ("server", "h2"),
            ("URL", "h3"),
            ("SeRvEr", "h4"),
        ];
        let user_variants = [("user", "u1"), ("username", "u2"), ("UserName", "u3")];
        let pass_variants = [
            ("pass", "p1"),
            ("password", "p2"),
            ("pwd", "p3"),
            ("PwD", "p4"),
        ];
        let db_variants = [
            ("dbname", "d1"),
            ("db", "d2"),
            ("database", "d3"),
            ("DbName", "d4"),
        ];

        for (hk, hv) in host_variants {
            let s = ConnectionSettings::from_map([(hk, hv), ("user", "u"), ("pass", "p")]).unwrap();
            assert_eq!(s.host, hv);
            assert_eq!(s.port, 3306);
        }
        for (uk, uv) in user_variants {
            let s = ConnectionSettings::from_map([("host", "h"), (uk, uv), ("pass", "p")]).unwrap();
            assert_eq!(s.user, uv);
        }
        for (pk, pv) in pass_variants {
            let s = ConnectionSettings::from_map([("host", "h"), ("user", "u"), (pk, pv)]).unwrap();
            assert_eq!(s.password.unwrap().expose_secret(), pv);
        }
        for (dk, dv) in db_variants {
            let s = ConnectionSettings::from_map([
                ("host", "h"),
                ("user", "u"),
                ("pass", "p"),
                (dk, dv),
                ("PoRt", "3307"),
                ("pool_size", "8"),
                ("custom_flag", "on"),
            ])
            .unwrap();
            assert_eq!(s.database.as_deref(), Some(dv));
            assert_eq!(s.port, 3307);
            assert_eq!(s.extra.get("pool_size").map(String::as_str), Some("8"));
            assert_eq!(s.extra.get("custom_flag").map(String::as_str), Some("on"));
        }
    }

    #[test]
    fn from_map_rejects_conflicting_synonyms_naming_both_keys() {
        let host_err = ConnectionSettings::from_map([
            ("host", "a.example"),
            ("SERVER", "b.example"),
            ("user", "u"),
            ("pass", "p"),
        ])
        .unwrap_err();
        assert_eq!(
            host_err,
            SettingsError::ConflictingSynonyms {
                field: "host",
                first_key: "host".into(),
                second_key: "SERVER".into(),
            }
        );
        let msg = host_err.to_string();
        assert!(msg.contains("host") && msg.contains("SERVER"), "{msg}");

        let pass_err = ConnectionSettings::from_map([
            ("host", "h"),
            ("user", "u"),
            ("pass", "secret-one"),
            ("PASSWORD", "secret-two"),
        ])
        .unwrap_err();
        let pass_msg = pass_err.to_string();
        assert!(
            pass_msg.contains("pass") && pass_msg.contains("PASSWORD"),
            "{pass_msg}"
        );
        assert!(
            !pass_msg.contains("secret-one") && !pass_msg.contains("secret-two"),
            "password leaked in conflict error: {pass_msg}"
        );
    }

    #[test]
    fn from_map_rejects_missing_required_keys_naming_the_key() {
        let no_host = ConnectionSettings::from_map([("user", "u"), ("pass", "p")]).unwrap_err();
        assert!(no_host.to_string().contains("host"), "{no_host}");

        let no_user = ConnectionSettings::from_map([("host", "h"), ("pass", "p")]).unwrap_err();
        assert!(no_user.to_string().contains("user"), "{no_user}");

        let no_pass = ConnectionSettings::from_map([("host", "h"), ("user", "u")]).unwrap_err();
        assert!(no_pass.to_string().contains("password"), "{no_pass}");
    }

    #[test]
    fn from_secrets_supports_prefix_defaults_and_synonyms() {
        let provider = MapProvider::new("vault")
            .with("SINGLESTORE_SERVER", "s2.example")
            .with("SINGLESTORE_USER", "admin")
            .with("SINGLESTORE_PWD", "secret")
            .with("OTHER_KEY", "ignored");
        let settings = ConnectionSettings::from_secrets(&provider)
            .prefix("SINGLESTORE_")
            .port(3308)
            .database("edw_db")
            .tls(TlsMode::VerifyFull)
            .build()
            .unwrap();
        assert_eq!(settings.host, "s2.example");
        assert_eq!(settings.port, 3308);
        assert_eq!(settings.user, "admin");
        assert_eq!(settings.password.unwrap().expose_secret(), "secret");
        assert_eq!(settings.database.as_deref(), Some("edw_db"));
        assert_eq!(settings.tls, TlsMode::VerifyFull);
        assert!(!settings.extra.contains_key("OTHER_KEY"));
    }

    #[test]
    fn custom_secret_provider_without_layers_override_and_db_port_precedence_guards() {
        use secrecy::SecretString;
        use vdp_secrets::SecretProvider;

        #[derive(Debug)]
        struct CustomLeaf(Vec<(&'static str, &'static str)>);

        impl SecretProvider for CustomLeaf {
            fn name(&self) -> String {
                "custom-leaf".into()
            }

            fn get(&self, key: &str) -> Option<SecretString> {
                self.0
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| SecretString::from((*v).to_owned()))
            }

            fn keys(&self) -> Vec<String> {
                self.0.iter().map(|(k, _)| (*k).to_owned()).collect()
            }
        }

        // Default `layers()` works on a custom implementor without overriding `layers`.
        let custom = CustomLeaf(vec![
            ("SINGLESTORE_HOST", "custom.example"),
            ("SINGLESTORE_USER", "alice"),
            ("SINGLESTORE_PASS", "pw"),
        ]);
        assert_eq!(custom.layers().len(), 1);
        let from_custom = ConnectionSettings::from_secrets(&custom)
            .prefix("SINGLESTORE_")
            .build()
            .unwrap();
        assert_eq!(from_custom.host, "custom.example");
        assert_eq!(from_custom.user, "alice");

        // `.database(...)` and `.port(...)` ignore secret database/port overrides (even conflicting ones).
        let with_overrides = CustomLeaf(vec![
            ("SINGLESTORE_HOST", "s2.example"),
            ("SINGLESTORE_USER", "admin"),
            ("SINGLESTORE_PASS", "pw"),
            ("SINGLESTORE_DB", "wrong_db"),
            ("SINGLESTORE_DATABASE", "wrong_db2"),
            ("SINGLESTORE_DBNAME", "wrong_db3"),
            ("SINGLESTORE_PORT", "1"),
        ]);
        let fixed = ConnectionSettings::from_secrets(&with_overrides)
            .prefix("SINGLESTORE_")
            .port(3308)
            .database("edw_db")
            .build()
            .unwrap();
        assert_eq!(fixed.database.as_deref(), Some("edw_db"));
        assert_eq!(fixed.port, 3308);
    }

    #[test]
    fn from_secrets_uses_first_of_precedence_across_layers_and_scopes_conflicts() {
        use vdp_secrets::{ChainProvider, SecretProvider};

        let make_env = || {
            MapProvider::new("env")
                .with("SINGLESTORE_HOST", "s2.example")
                .with("SINGLESTORE_USER", "admin")
                .with("SINGLESTORE_PASS", "from-env")
        };
        let make_vault = || {
            MapProvider::new("vault")
                .with("SINGLESTORE_PASSWORD", "from-vault")
                .with("SINGLESTORE_DB", "vault_db")
        };
        for layers in [
            vec![
                Box::new(make_env()) as Box<dyn SecretProvider>,
                Box::new(make_vault()),
            ],
            vec![
                Box::new(make_vault()) as Box<dyn SecretProvider>,
                Box::new(make_env()),
            ],
        ] {
            let chain = ChainProvider::new(layers);
            let resolved = ConnectionSettings::from_secrets(&chain)
                .prefix("SINGLESTORE_")
                .port(3306)
                .database("edw_dv_rdv")
                .build()
                .unwrap();
            assert_eq!(
                resolved.password.as_ref().unwrap().expose_secret(),
                "from-env"
            );
            assert_eq!(resolved.database.as_deref(), Some("edw_dv_rdv"));
            assert_eq!(resolved.port, 3306);
        }

        let single_layer_conflict = MapProvider::new("env")
            .with("SINGLESTORE_HOST", "s2.example")
            .with("SINGLESTORE_USER", "admin")
            .with("SINGLESTORE_PASS", "one")
            .with("SINGLESTORE_PASSWORD", "two");
        let err = ConnectionSettings::from_secrets(&single_layer_conflict)
            .prefix("SINGLESTORE_")
            .build()
            .unwrap_err();
        assert!(
            matches!(
                err,
                SettingsError::ConflictingSynonyms {
                    field: "password",
                    ..
                }
            ),
            "unexpected error: {err:?}"
        );
    }
}
