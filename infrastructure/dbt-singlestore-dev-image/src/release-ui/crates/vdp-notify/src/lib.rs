//! Release announcements.
//!
//! A building block of the release flow and the release hub: it renders the release candidate overview and sends it.
//!
//! # Responsibilities
//!
//! - [`Announcement`] renders the release candidate overview (subject, HTML, plain text) for an analysis and its
//!   selection, identical to the Python `ReleaseEmailBuilder` output.
//! - [`Mailer`] sends it; [`SmtpMailer`] uses lettre with the `[notify]` settings and the SMTP account from the vdp
//!   secrets.
//! - Without an account it reports [`Delivery::PreviewOnly`] instead of failing, as the Python UI did.
//!
//! # Example
//!
//! ```
//! use vdp_notify::{Announcement, AnnouncementInput, AnnouncementRow, Delivery, Mailer, NotifySettings, SmtpMailer};
//! use vdp_secrets::MapProvider;
//!
//! let row = AnnouncementRow { id: "INTEL-1".into(), summary: "Add orders".into(), ..AnnouncementRow::default() };
//! let announcement = Announcement::render(AnnouncementInput {
//!     fix_version: "2026_DROP05",
//!     branch: "release/2026_DROP05",
//!     included: vec![row],
//!     excluded: Vec::new(),
//! });
//! assert!(announcement.subject.contains("2026_DROP05"));
//!
//! // No SMTP account in the secrets: the announcement is only previewed.
//! let mailer = SmtpMailer::new(NotifySettings::default(), &MapProvider::new("none"));
//! let delivery = mailer.send(&announcement, &["team@example.com".to_owned()])?;
//! assert!(matches!(delivery, Delivery::PreviewOnly { .. }));
//! # Ok::<(), vdp_notify::NotifyError>(())
//! ```

mod announcement;
mod mail;
mod settings;

pub use announcement::{Announcement, AnnouncementInput, AnnouncementRow};
pub use mail::{Delivery, Mailer, NotifyError, SmtpMailer};
pub use settings::{NotifySettings, SmtpSecurity};
