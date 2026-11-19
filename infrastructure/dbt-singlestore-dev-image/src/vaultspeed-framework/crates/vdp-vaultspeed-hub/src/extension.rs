//! `/api/v1/vaultspeed/...` as a [`HubExtension`]: the route table of the VaultSpeed side. Reading needs a
//! sign-in; each write needs the GitLab right on the branch it changes (push, or force push for resets and
//! recreated releases).

use std::sync::Arc;

use vdp_jira::IssueTracker;
use vdp_release::Resolution;
use vdp_release_hub::http::{API, Reply};
use vdp_release_hub::{
    Actor, ActorRequest, ExtensionRequest, GitLab, HubError, HubExtension, Need, api::CloneRequest,
    auth::BranchRight,
};

use crate::api::{VsCommitCreate, VsMergeCreate, VsReleaseCreate, VsResetCreate};
use crate::hub::{ChangeFileRequest, VsConfig, VsHub};

/// The URL segment, and the key in the hub's status and in a signed-in user's rights.
pub const SEGMENT: &str = "vaultspeed";

impl VsHub {
    /// Who a write on `branch` acts as: the GitLab right on the rule that covers it, pushing to the framework
    /// project as the signed-in person.
    fn actor_for(
        &self,
        request: &ExtensionRequest<'_>,
        branch: &str,
        need: Need,
    ) -> Result<Actor, HubError> {
        request.actor(&ActorRequest {
            branch,
            rights_branch: &self.rights_branch(branch),
            need,
            push_to: self.config().gitlab.as_ref(),
        })
    }

    /// Changing the clone itself (clone, repair, decisions on a waiting job): any push right in the repository.
    fn maintainer(&self, request: &ExtensionRequest<'_>) -> Result<Actor, HubError> {
        let main = self.rules().settings.main.clone();
        self.actor_for(request, &main, Need::Push).or_else(|first| {
            self.rules()
                .environments()
                .iter()
                .find_map(|env| self.actor_for(request, env, Need::Push).ok())
                .ok_or(first)
        })
    }
}

/// The VaultSpeed side as the hub's extension: register `Arc::new(VaultspeedExtension::new(..))`.
#[derive(Debug, Clone)]
pub struct VaultspeedExtension {
    vs: Arc<VsHub>,
}

impl VaultspeedExtension {
    /// Creates the VaultSpeed side of the hub from its configuration and, optionally, a Jira tracker for ticket details.
    pub fn new(config: VsConfig, tracker: Option<Arc<dyn IssueTracker>>) -> Self {
        Self {
            vs: VsHub::new(config, tracker),
        }
    }

    /// The side itself (its clone, jobs and flows), for callers besides the HTTP routes.
    pub fn hub(&self) -> &Arc<VsHub> {
        &self.vs
    }
}

impl HubExtension for VaultspeedExtension {
    fn segment(&self) -> &str {
        SEGMENT
    }

    fn summary(&self) -> serde_json::Value {
        serde_json::to_value(self.vs.summary()).unwrap_or(serde_json::Value::Null)
    }

    /// Per branch of the framework repository; a GitLab error refuses everything (with the reason).
    fn rights(&self, hub_gitlab: &GitLab, token: &str) -> Vec<BranchRight> {
        let samples = self.vs.rights_samples();
        let gitlab = self
            .vs
            .config()
            .gitlab
            .clone()
            .unwrap_or_else(|| hub_gitlab.clone());
        let rights = gitlab
            .user(token)
            .and_then(|user| gitlab.rights(token, &user, &samples));
        match rights {
            Ok(rights) => rights,
            Err(error) => samples
                .into_iter()
                .map(|(branch, _)| BranchRight {
                    branch,
                    push: false,
                    force: false,
                    reason: Some(format!(
                        "{}: {}",
                        gitlab.project,
                        vdp_vaultspeed_domain::display_chain(&error)
                    )),
                })
                .collect(),
        }
    }

    #[allow(clippy::too_many_lines, reason = "one route table")]
    fn handle(&self, request: &ExtensionRequest<'_>) -> Reply {
        let vs = &self.vs;
        let param = |name: &str| request.param(name);
        let maintainer = || vs.maintainer(request);
        let jobs_after = || {
            request
                .param_opt("after")
                .and_then(|a| a.parse().ok())
                .unwrap_or(0)
        };
        match (request.method, request.segments) {
            ("GET", ["status"]) => Reply::from(200, vs.status(param("jira") == "refresh")),
            ("POST", ["repo", "clone"]) => Reply::from(
                202,
                maintainer().and_then(|_| {
                    request
                        .json::<CloneRequest>()
                        .and_then(|r| vs.clone_repo(&r))
                }),
            ),
            ("POST", ["repo", "sync"]) => Reply::from(202, vs.sync()),
            ("POST", ["repo", "repair"]) => {
                Reply::from(202, maintainer().and_then(|_| vs.repair()))
            }
            ("POST", ["repo", "reclone"]) => Reply::from(
                202,
                maintainer()
                    .and_then(|_| request.json::<CloneRequest>().and_then(|r| vs.reclone(&r))),
            ),
            ("GET", ["pipeline"]) => Reply::from(
                200,
                request
                    .host
                    .pipeline(request.session, &param("sha"), vs.config().gitlab.as_ref()),
            ),
            ("GET", ["file"]) => Reply::from(200, vs.file(&param("branch"), &param("path"))),
            ("GET", ["changes"]) => Reply::from(200, vs.changes(&param("from"), &param("to"))),
            ("GET", ["changes", "file"]) => Reply::from(
                200,
                vs.change_file(&ChangeFileRequest {
                    from: &param("from"),
                    to: &param("to"),
                    path: &param("path"),
                    entry: request.param_opt("entry").as_deref(),
                }),
            ),
            ("GET", ["tree"]) => Reply::from(200, vs.tree(&param("branch"), &param("dir"))),
            ("GET", ["history"]) => Reply::from(
                200,
                vs.history(&param("branch"), param("limit").parse().unwrap_or(30)),
            ),
            ("POST", ["releases"]) => Reply::from(
                202,
                request.json::<VsReleaseCreate>().and_then(|create| {
                    let branch = vs
                        .rules()
                        .release_branch(&create.request.drop)
                        .map_err(|e| {
                            HubError::BadRequest(vdp_vaultspeed_domain::display_chain(&e))
                        })?;
                    let need = if create.request.recreate {
                        Need::Force
                    } else {
                        Need::Push
                    };
                    let actor = vs.actor_for(request, &branch, need)?;
                    vs.create_release(&create, actor)
                }),
            ),
            ("POST", ["merges"]) => Reply::from(
                202,
                request.json::<VsMergeCreate>().and_then(|create| {
                    let actor = vs.actor_for(request, &create.request.environment, Need::Push)?;
                    vs.create_merge(&create, actor)
                }),
            ),
            ("POST", ["commits"]) => Reply::from(
                202,
                request.json::<VsCommitCreate>().and_then(|create| {
                    let actor = vs.actor_for(request, &create.request.branch, Need::Push)?;
                    vs.create_commit(&create, actor)
                }),
            ),
            ("POST", ["resets"]) => Reply::from(
                202,
                request.json::<VsResetCreate>().and_then(|create| {
                    let actor = vs.actor_for(request, &create.request.branch, Need::Force)?;
                    vs.create_reset(&create, actor)
                }),
            ),
            ("GET", ["jobs"]) => Reply::json(200, &vs.jobs()),
            ("POST", ["jobs", id, "resolution"]) => Reply::from(
                200,
                maintainer().and_then(|_| {
                    request
                        .json::<Resolution>()
                        .and_then(|r| vs.resolve(id, r).and_then(|()| vs.job(id, 0)))
                }),
            ),
            ("GET", ["jobs", id, "conflict"]) => match request.param_opt("path") {
                Some(path) => Reply::from(200, vs.conflict_versions(id, &path)),
                None => Reply::error(&HubError::BadRequest("path is required".into())),
            },
            ("GET", ["jobs", id]) => Reply::from(200, vs.job(id, jobs_after())),
            (method, segments) => Reply::error(&HubError::NotFound(format!(
                "{method} {API}/{SEGMENT}/{}",
                segments.join("/")
            ))),
        }
    }
}
