use serde::{Deserialize, Serialize};

/// How the SMTP connection is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// Plain connection upgraded with STARTTLS (port 587; what Python did).
    Starttls,
    /// TLS from the start (port 465).
    Tls,
    /// Unencrypted and unauthenticated: a local relay or a test server only.
    None,
}

/// `[notify]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct NotifySettings {
    /// Python hard-coded smtp.gmail.com:587 with STARTTLS; set the deployment's relay here.
    pub smtp_host: String,
    pub smtp_port: u16,
    pub security: SmtpSecurity,
    /// The sender. Empty: the `from_secret`, else the SMTP account.
    pub from: String,
    /// Secrets with the SMTP account, its password and an optional sender (dotenv, Vault or the environment).
    pub user_secret: String,
    pub password_secret: String,
    pub from_secret: String,
    /// Recipients the release hub proposes.
    pub default_recipients: Vec<String>,
    /// Seconds before an SMTP connection gives up.
    pub timeout_secs: u64,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: 587,
            security: SmtpSecurity::Starttls,
            from: String::new(),
            user_secret: "SMTP_USER".into(),
            password_secret: "SMTP_PASSWORD".into(),
            from_secret: "SMTP_FROM".into(),
            default_recipients: Vec::new(),
            timeout_secs: 15,
        }
    }
}

impl NotifySettings {
    pub const SECTION: &'static str = "notify";
}
