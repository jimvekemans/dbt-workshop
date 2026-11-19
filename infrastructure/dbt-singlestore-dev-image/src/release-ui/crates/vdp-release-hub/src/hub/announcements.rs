//! Release announcement drafting and delivery on [`Hub`].

use vdp_notify::Announcement;

use super::Hub;
use crate::api::{AnnouncementSend, AnnouncementSent};
use crate::error::HubError;

impl Hub {
    /// The announcement for analysis `id` and its current selection, for `branch` (default: the analysis' branch).
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such analysis.
    pub fn announcement(&self, id: &str, branch: Option<&str>) -> Result<Announcement, HubError> {
        let stored = self.stored(id)?;
        let branch = branch
            .filter(|b| !b.trim().is_empty())
            .unwrap_or(&stored.branch);
        Ok(Announcement::for_analysis(
            &stored.analysis,
            &stored.selection,
            branch,
        ))
    }

    /// Sends the announcement to `send.recipients`; without an SMTP account it is only previewed.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such analysis; [`HubError::BadRequest`] when there are no recipients or an
    /// address is invalid; [`HubError::Internal`] when the message cannot be built or the SMTP server fails.
    pub fn send_announcement(&self, send: &AnnouncementSend) -> Result<AnnouncementSent, HubError> {
        let announcement = self.announcement(&send.analysis_id, send.branch.as_deref())?;
        let delivery = self
            .mailer
            .send(&announcement, &send.recipients)
            .map_err(|e| match e {
                vdp_notify::NotifyError::NoRecipients | vdp_notify::NotifyError::Address(..) => {
                    HubError::BadRequest(vdp_core::display_chain(&e))
                }
                other => HubError::Internal(vdp_core::display_chain(&other)),
            })?;
        Ok(AnnouncementSent {
            delivery,
            announcement,
        })
    }
}
