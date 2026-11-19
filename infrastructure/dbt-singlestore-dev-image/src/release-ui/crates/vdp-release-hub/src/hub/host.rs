//! Extension host bridge scoping extension rights lookups under a registered segment.

use super::{Actor, Hub};
use crate::auth::GitLab;
use crate::error::HubError;
use crate::extension::{ActorRequest, ExtensionHost, Need};

/// The hub as one extension sees it: rights are looked up under the extension's own segment.
pub(super) struct Host<'a> {
    pub(super) hub: &'a Hub,
    pub(super) segment: &'a str,
}

impl ExtensionHost for Host<'_> {
    fn actor(&self, session: Option<&str>, request: &ActorRequest<'_>) -> Result<Actor, HubError> {
        let ActorRequest {
            branch,
            rights_branch,
            need,
            push_to,
        } = *request;
        let hub = self.hub;
        let Some(hub_gitlab) = &hub.config.gitlab else {
            // No sign-in (local use): the clone's identity, pushing to origin.
            return hub.actor(session, false);
        };
        let session = session
            .and_then(|id| hub.sessions.get(id))
            .ok_or_else(|| HubError::Unauthorized("sign in with your GitLab token".into()))?;
        let right = session
            .user
            .right(self.segment, rights_branch)
            .cloned()
            .ok_or_else(|| {
                HubError::Forbidden(format!("no rights known for {branch}: sign in again"))
            })?;
        let allowed = match need {
            Need::Push => right.push,
            Need::Force => right.force,
        };
        if !allowed {
            return Err(HubError::Forbidden(right.reason.unwrap_or_else(|| {
                format!(
                    "you may not {} {branch}",
                    if need == Need::Force {
                        "force-push"
                    } else {
                        "push to"
                    }
                )
            })));
        }
        if session.user.email.is_empty() {
            return Err(HubError::Forbidden(
                "your GitLab account has no commit email: set one, commits need it".into(),
            ));
        }
        Ok(Actor {
            identity: Some(session.identity()),
            push: session.push_target(push_to.unwrap_or(hub_gitlab)),
        })
    }

    fn pipeline(
        &self,
        session: Option<&str>,
        sha: &str,
        project: Option<&GitLab>,
    ) -> Result<Option<crate::auth::Pipeline>, HubError> {
        let hub = self.hub;
        let Some(hub_gitlab) = &hub.config.gitlab else {
            return Ok(None);
        };
        let session = session
            .and_then(|id| hub.sessions.get(id))
            .ok_or_else(|| HubError::Unauthorized("sign in with your GitLab token".into()))?;
        project.unwrap_or(hub_gitlab).pipeline(session.token(), sha)
    }
}
