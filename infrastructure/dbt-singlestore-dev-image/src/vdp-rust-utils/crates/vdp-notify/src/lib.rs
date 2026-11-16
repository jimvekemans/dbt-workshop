//! Release announcements.
//!
//! - [`Announcement`] renders the release candidate overview (subject, HTML, plain text) for an analysis and
//!   its selection, identical to the Python `ReleaseEmailBuilder` output.
//! - [`Mailer`] sends it; [`SmtpMailer`] uses lettre with the `[notify]` settings and the SMTP account from the
//!   vdp secrets. Without an account it reports [`Delivery::PreviewOnly`] instead of failing, as the Python UI
//!   did.

mod announcement;
mod mail;
mod settings;

pub use announcement::{Announcement, AnnouncementRow};
pub use mail::{Delivery, Mailer, NotifyError, SmtpMailer};
pub use settings::{NotifySettings, SmtpSecurity};
