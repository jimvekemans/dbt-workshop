//! The plug-in boundary of the hub: a second repository steered from the same UI, served below its own URL
//! segment (`/api/v1/<segment>/...`).
//!
//! The hub core knows nothing about what an extension does. It only owns what must stay in one place for every
//! side: sessions and GitLab sign-in, the JSON error shape, and the status and user documents the UI loads
//! first. An extension therefore returns plain data for those (`serde_json::Value` for its summary, branch
//! rights for the signed-in user) and receives the request as it came in, together with a handle on the hub to
//! resolve who may write. Its own jobs, clone and settings stay in its own crate.

use std::fmt::Debug;

use serde::de::DeserializeOwned;

use crate::auth::{BranchRight, GitLab, Pipeline};
use crate::error::HubError;
use crate::http::{Reply, parse_body, query_param};
use crate::hub::Actor;

/// What a write needs on a branch: a push, or a force push (resets and recreated releases).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Push,
    Force,
}

/// A repository the hub steers next to its own, behind `/api/v1/<segment>/...`.
pub trait HubExtension: Send + Sync + Debug {
    /// The URL segment, and the key of this extension in the status (`summary`) and in the signed-in user's
    /// `rights`, e.g. `"vaultspeed"`.
    fn segment(&self) -> &str;

    /// The extension at a glance, as the status document shows it under [`HubExtension::segment`].
    fn summary(&self) -> serde_json::Value;

    /// What the person behind `token` may do on the extension's branches. `gitlab` is the hub's own project:
    /// the fallback for an extension without a GitLab project of its own. Errors are part of the answer (every
    /// branch refused, with the reason), so sign-in itself does not fail on them.
    fn rights(&self, gitlab: &GitLab, token: &str) -> Vec<BranchRight>;

    /// Answers one request below the segment. The hub has already checked the sign-in (when it is on).
    fn handle(&self, request: &ExtensionRequest<'_>) -> Reply;
}

/// One request for an extension: what came after `/api/v1/<segment>`.
#[derive(Clone, Copy)]
pub struct ExtensionRequest<'a> {
    pub method: &'a str,
    /// The path below the segment, decoded and split at `/`.
    pub segments: &'a [&'a str],
    /// The query string without the `?`, undecoded.
    pub query: &'a str,
    pub body: &'a [u8],
    /// The session cookie, for [`ExtensionHost`].
    pub session: Option<&'a str>,
    pub host: &'a dyn ExtensionHost,
}

impl Debug for ExtensionRequest<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionRequest")
            .field("method", &self.method)
            .field("segments", &self.segments)
            .field("query", &self.query)
            .finish_non_exhaustive()
    }
}

/// Parameters for resolving who an extension write acts as.
#[derive(Debug, Clone, Copy)]
pub struct ActorRequest<'a> {
    pub branch: &'a str,
    pub rights_branch: &'a str,
    pub need: Need,
    pub push_to: Option<&'a GitLab>,
}

impl ExtensionRequest<'_> {
    /// A query parameter, decoded; empty when absent.
    pub fn param(&self, name: &str) -> String {
        query_param(self.query, name).unwrap_or_default()
    }

    /// A query parameter, decoded; `None` when absent.
    pub fn param_opt(&self, name: &str) -> Option<String> {
        query_param(self.query, name)
    }

    /// The JSON body as `T` (an empty body counts as `{}`); 400 when it does not fit.
    ///
    /// # Errors
    ///
    /// [`HubError::BadRequest`] when the body is not valid JSON for `T`.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, HubError> {
        parse_body(self.body)
    }

    /// Who a write on `request.branch` acts as: see [`ExtensionHost::actor`].
    ///
    /// # Errors
    ///
    /// As [`ExtensionHost::actor`].
    pub fn actor(&self, request: &ActorRequest<'_>) -> Result<Actor, HubError> {
        self.host.actor(self.session, request)
    }
}

/// What the hub offers an extension while it handles a request.
pub trait ExtensionHost {
    /// Who a write acts as. With GitLab sign-in: the signed-in person, who must hold the extension's right for
    /// `request.rights_branch` (the rule that covers `request.branch`, e.g. `feature/*`) and will push to
    /// `request.push_to` (the extension's project; `None`: the hub's own) as themselves. Without sign-in
    /// (local use): the clone identity of the hub, pushing to origin.
    ///
    /// # Errors
    ///
    /// [`HubError::Unauthorized`] when sign-in is on and the session is missing or expired; [`HubError::Forbidden`] when
    /// the person lacks the right for `request.rights_branch`; [`HubError::Internal`] when, without sign-in, the clone's
    /// identity cannot be read.
    fn actor(&self, session: Option<&str>, request: &ActorRequest<'_>) -> Result<Actor, HubError>;

    /// The newest CI pipeline of `project` (`None`: the hub's own) for `sha`, read with the signed-in person's
    /// token. `Ok(None)` without sign-in.
    ///
    /// # Errors
    ///
    /// As [`GitLab::pipeline`]: [`HubError::BadRequest`] when `sha` is not a commit id; [`HubError::Unauthorized`],
    /// [`HubError::Forbidden`] or [`HubError::Internal`] when GitLab refuses, denies or cannot be reached.
    fn pipeline(
        &self,
        session: Option<&str>,
        sha: &str,
        project: Option<&GitLab>,
    ) -> Result<Option<Pipeline>, HubError>;
}
