//! Sending announcements.

use std::fmt::Debug;
use std::time::Duration;

use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::{Deserialize, Serialize};
use vdp_secrets::{ExposeSecret, SecretProvider};

use crate::announcement::Announcement;
use crate::settings::{NotifySettings, SmtpSecurity};

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("no recipients")]
    NoRecipients,
    #[error("invalid address '{0}': {1}")]
    Address(String, String),
    #[error("could not build the message: {0}")]
    Message(String),
    #[error("SMTP: {0}")]
    Smtp(String),
}

/// What happened to an announcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Delivery {
    Sent {
        recipients: Vec<String>,
    },
    /// SMTP is not configured: nothing was sent (the caller shows the preview).
    PreviewOnly {
        reason: String,
    },
}

/// Sends announcements.
pub trait Mailer: Send + Sync + Debug {
    /// Whether [`Mailer::send`] can deliver, and if not, why.
    fn readiness(&self) -> Result<(), String>;

    fn send(
        &self,
        announcement: &Announcement,
        recipients: &[String],
    ) -> Result<Delivery, NotifyError>;
}

/// SMTP through lettre (synchronous, rustls).
pub struct SmtpMailer {
    settings: NotifySettings,
    credentials: Option<(String, String)>,
    from: Option<String>,
}

impl Debug for SmtpMailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpMailer")
            .field("host", &self.settings.smtp_host)
            .field("port", &self.settings.smtp_port)
            .field("security", &self.settings.security)
            .field("credentials", &self.credentials.as_ref().map(|_| "<set>"))
            .field("from", &self.from)
            .finish()
    }
}

impl SmtpMailer {
    /// Reads the account and sender from the secrets named in the settings.
    pub fn new(settings: NotifySettings, secrets: &dyn SecretProvider) -> Self {
        let get = |key: &str| {
            secrets
                .get(key)
                .map(|v| v.expose_secret().trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let credentials = get(&settings.user_secret).zip(get(&settings.password_secret));
        let from = Some(settings.from.trim().to_owned())
            .filter(|f| !f.is_empty())
            .or_else(|| get(&settings.from_secret))
            .or_else(|| credentials.as_ref().map(|(user, _)| user.clone()));
        Self {
            settings,
            credentials,
            from,
        }
    }

    fn transport(&self) -> Result<SmtpTransport, NotifyError> {
        let host = &self.settings.smtp_host;
        let smtp = |e: lettre::transport::smtp::Error| NotifyError::Smtp(e.to_string());
        let mut builder = match self.settings.security {
            SmtpSecurity::Starttls => SmtpTransport::starttls_relay(host).map_err(smtp)?,
            SmtpSecurity::Tls => SmtpTransport::relay(host).map_err(smtp)?,
            SmtpSecurity::None => SmtpTransport::builder_dangerous(host),
        }
        .port(self.settings.smtp_port)
        .timeout(Some(Duration::from_secs(self.settings.timeout_secs)));
        if let Some((user, password)) = &self.credentials {
            builder = builder.credentials(Credentials::new(user.clone(), password.clone()));
        }
        Ok(builder.build())
    }
}

fn mailbox(address: &str) -> Result<Mailbox, NotifyError> {
    address
        .trim()
        .parse()
        .map_err(|e: lettre::address::AddressError| {
            NotifyError::Address(address.to_owned(), e.to_string())
        })
}

impl Mailer for SmtpMailer {
    fn readiness(&self) -> Result<(), String> {
        if self.settings.smtp_host.trim().is_empty() {
            return Err("[notify].smtp_host is empty".into());
        }
        if self.settings.security != SmtpSecurity::None && self.credentials.is_none() {
            return Err(format!(
                "SMTP credentials not configured ({} / {})",
                self.settings.user_secret, self.settings.password_secret
            ));
        }
        if self.from.is_none() {
            return Err(format!(
                "no sender: set [notify].from or {}",
                self.settings.from_secret
            ));
        }
        Ok(())
    }

    fn send(
        &self,
        announcement: &Announcement,
        recipients: &[String],
    ) -> Result<Delivery, NotifyError> {
        let recipients: Vec<String> = recipients
            .iter()
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty())
            .collect();
        if recipients.is_empty() {
            return Err(NotifyError::NoRecipients);
        }
        if let Err(reason) = self.readiness() {
            tracing::info!(%reason, "SMTP not configured; preview only");
            return Ok(Delivery::PreviewOnly { reason });
        }
        let from = self.from.as_deref().unwrap_or_default();
        let mut builder = Message::builder()
            .from(mailbox(from)?)
            .subject(&announcement.subject);
        for recipient in &recipients {
            builder = builder.to(mailbox(recipient)?);
        }
        let message = builder
            .multipart(MultiPart::alternative_plain_html(
                announcement.text.clone(),
                announcement.html.clone(),
            ))
            .map_err(|e| NotifyError::Message(e.to_string()))?;
        self.transport()?
            .send(&message)
            .map_err(|e| NotifyError::Smtp(e.to_string()))?;
        tracing::info!(count = recipients.len(), "announcement sent");
        Ok(Delivery::Sent { recipients })
    }
}
