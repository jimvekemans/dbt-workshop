use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use secrecy::SecretString;

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
}

impl ConnectionSettings {
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
            connect_timeout: Duration::from_secs(30),
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

impl fmt::Debug for ConnectionSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionSettings")
            .field("endpoint", &self.endpoint())
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("tls", &self.tls)
            .field("ca_certificate", &self.ca_certificate)
            .field("client_certificate", &self.client_certificate)
            .field("connect_timeout", &self.connect_timeout)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
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
    }
}
