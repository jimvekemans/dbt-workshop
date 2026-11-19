//! Structured lifecycle milestone events for console and JSON run logs.

use std::fmt::{self, Display};

/// Tracing target for all structured lifecycle milestone events.
pub const TARGET: &str = "vdp::milestone";

/// Standardized lifecycle milestones emitted as structured `tracing` events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Milestone {
    Initialized,
    Attached,
    Skipped,
    Preview,
    Completed,
    Failed,
}

impl Milestone {
    /// Canonical lowercase name recorded in the `milestone` tracing field.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialized => "initialized",
            Self::Attached => "attached",
            Self::Skipped => "skipped",
            Self::Preview => "preview",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl Display for Milestone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Emits an `INFO` tracing event carrying the structured `milestone` and `subject` fields.
pub fn emit(milestone: Milestone, subject: &str, detail: impl Display) {
    tracing::info!(
        target: TARGET,
        milestone = milestone.as_str(),
        subject,
        "{detail}"
    );
}

/// Emits [`Milestone::Initialized`].
pub fn initialized(subject: &str, detail: impl Display) {
    emit(Milestone::Initialized, subject, detail);
}

/// Emits [`Milestone::Initialized`] at `DEBUG` level (for lazy service initialization).
pub fn initialized_debug(subject: &str, detail: impl Display) {
    tracing::debug!(
        target: TARGET,
        milestone = Milestone::Initialized.as_str(),
        subject,
        "{detail}"
    );
}

/// Emits [`Milestone::Attached`].
pub fn attached(subject: &str, detail: impl Display) {
    emit(Milestone::Attached, subject, detail);
}

/// Emits [`Milestone::Skipped`].
pub fn skipped(subject: &str, detail: impl Display) {
    emit(Milestone::Skipped, subject, detail);
}

/// Emits [`Milestone::Preview`].
pub fn preview(subject: &str, detail: impl Display) {
    emit(Milestone::Preview, subject, detail);
}

/// Emits [`Milestone::Completed`].
pub fn completed(subject: &str, detail: impl Display) {
    emit(Milestone::Completed, subject, detail);
}

/// Emits [`Milestone::Failed`] at `WARN` level.
pub fn failed(subject: &str, detail: impl Display) {
    tracing::warn!(
        target: TARGET,
        milestone = Milestone::Failed.as_str(),
        subject,
        "{detail}"
    );
}
