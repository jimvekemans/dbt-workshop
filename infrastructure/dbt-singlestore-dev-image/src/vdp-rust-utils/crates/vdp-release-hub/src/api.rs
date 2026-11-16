//! The request and response bodies of the API (`/api/v1`). Their TypeScript twins are generated with the `ts`
//! feature, so the frontend is type-checked against them.

use serde::{Deserialize, Serialize};
use vdp_notify::{Announcement, Delivery};
use vdp_release::{
    Analysis, AnalysisKind, OnConflictChoice, OriginState, PromotionKind, StrategyChoice,
};

use crate::jobs::Job;

/// The clone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RepoStatus {
    pub dir: String,
    pub cloned: bool,
    pub origin_url: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    /// When origin was last fetched (ms since the epoch).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub last_fetch: Option<i64>,
    /// What blocks or endangers the next job (empty: healthy). Not checked while a job runs.
    pub health: Vec<HealthProblem>,
}

/// One thing wrong with the clone; `POST /repo/repair` fixes all of them (or re-clone when `broken`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HealthProblem {
    /// `in_progress`, `index_lock`, `dirty`, `local_branches` or `broken`.
    pub code: String,
    pub detail: String,
}

/// The git identity commits are made with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

/// Whether an integration can be used, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Readiness {
    pub ready: bool,
    pub detail: Option<String>,
}

impl Readiness {
    pub fn from_result(result: Result<(), String>) -> Self {
        match result {
            Ok(()) => Self {
                ready: true,
                detail: None,
            },
            Err(detail) => Self {
                ready: false,
                detail: Some(detail),
            },
        }
    }
}

/// The defaults the UI proposes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Defaults {
    pub repo_url: String,
    pub source: String,
    pub main: String,
    pub branch_prefix: String,
    pub ticket_prefix: String,
    pub recipients: Vec<String>,
}

/// `GET /status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Status {
    pub version: String,
    /// `gitlab`: sign in with a GitLab token; `none`: no sign-in (local use).
    pub auth: String,
    /// Who is signed in (with `auth: gitlab`).
    pub user: Option<crate::auth::SessionUser>,
    pub repo: RepoStatus,
    pub identity: Option<GitIdentity>,
    pub jira: Readiness,
    pub smtp: Readiness,
    pub active_job: Option<Job>,
    pub defaults: Defaults,
    /// The VaultSpeed side, when the hub steers the framework repository.
    pub vaultspeed: Option<VsSummary>,
}

/// The VaultSpeed side at a glance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct VsSummary {
    pub repo: RepoStatus,
    pub repo_url: String,
    pub active_job: Option<Job>,
    /// Environment branches, lowest first.
    pub environments: Vec<String>,
    pub direct_merge_environments: Vec<String>,
    pub main: String,
    pub zip_prefix: String,
    pub release_prefix: String,
    pub pml_dir: String,
}

/// How a VaultSpeed run handles conflicts, and whether it only previews.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VsRun {
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dry_run: Option<bool>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub on_conflict: Option<OnConflictChoice>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub strategy: Option<StrategyChoice>,
}

/// `POST /vaultspeed/releases`: build or extend `feature/<drop>`. `expect`: the branch head the preview saw
/// (`null`: it did not exist); ignored in a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VsReleaseCreate {
    pub request: vdp_vaultspeed_release::flows::BuildRequest,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub expect: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub run: Option<VsRun>,
}

/// `POST /vaultspeed/merges`. `expect`: the environment's head the preview saw. `plan`: also plan what CI would
/// deploy (offline, on the merged tree).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VsMergeCreate {
    pub request: vdp_vaultspeed_release::flows::MergeRequest,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub expect: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub plan: Option<bool>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub run: Option<VsRun>,
}

/// `POST /vaultspeed/commits`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VsCommitCreate {
    pub request: vdp_vaultspeed_release::flows::CommitRequest,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub expect: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dry_run: Option<bool>,
}

/// `POST /vaultspeed/resets`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct VsResetCreate {
    pub request: vdp_vaultspeed_release::flows::ResetRequest,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub expect: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dry_run: Option<bool>,
}

/// `GET /vaultspeed/file?branch=&path=`: a file as origin has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct VsFile {
    pub branch: String,
    pub path: String,
    /// `None`: the branch has no such file.
    pub content: Option<String>,
    pub head: String,
}

/// `POST /repo/clone`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct CloneRequest {
    /// Default: `[release_hub].repo_url`.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
}

/// `POST /analyses`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct AnalysisCreate {
    pub kind: AnalysisKind,
    /// The target DROP. For a patch it defaults to the release branch's suffix.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fix_version: Option<String>,
    /// Default: `[release].from`.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub source: Option<String>,
    /// A release: default `[release].to`. A patch: the release branch (required).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub compare: Option<String>,
    /// Keep tickets without a DROP fix version by default (the CLI's `--keep-empty-fixversions`).
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub keep_empty: Option<bool>,
}

/// A stored analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnalysisView {
    pub id: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created: i64,
    /// The release branch it is for: proposed for a release, the patched branch for a patch.
    pub branch: String,
    pub analysis: Analysis,
    /// Reverted (release) or added (patch).
    pub selection: Vec<String>,
    /// The source or compare branch moved since: execute is refused.
    pub stale: bool,
    /// Bumped by every selection change: send it back so two people do not overwrite each other's board.
    pub version: u32,
}

/// One line of `GET /analyses`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnalysisSummary {
    pub id: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created: i64,
    pub kind: AnalysisKind,
    pub fix_version: String,
    pub branch: String,
    pub tickets: u32,
    pub selected: u32,
}

/// `PUT /analyses/{id}/selection`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct SelectionUpdate {
    pub selection: Vec<String>,
    /// The `version` the board was loaded at; a newer one on the server is a 409.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub version: Option<u32>,
}

/// `POST /releases`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct ReleaseCreate {
    pub analysis_id: String,
    /// Default: the analysis' branch.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub branch: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dry_run: Option<bool>,
    /// Recreate the branch and push with a lease on what origin has.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub force: Option<bool>,
    /// The `origin` of the preview's outcome: executing refuses if origin moved since.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub expect: Option<OriginState>,
    /// The preview's `losses` (commits only the old branch has) were shown and accepted.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub accept_losses: Option<bool>,
    /// Pause on a conflict (default) or resolve it for the reverted side automatically.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub on_conflict: Option<OnConflictChoice>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub strategy: Option<StrategyChoice>,
}

/// `POST /patches`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct PatchCreate {
    pub analysis_id: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dry_run: Option<bool>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub on_conflict: Option<OnConflictChoice>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub strategy: Option<StrategyChoice>,
}

/// `GET /jobs/{id}/conflict?path=`: the three versions of a conflicting file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ConflictVersionsView {
    pub path: String,
    /// The common ancestor.
    pub base: Option<String>,
    /// The release branch as it is.
    pub branch: Option<String>,
    /// What the step would make of it (the revert's or the pick's version).
    pub change: Option<String>,
    pub binary: bool,
    pub truncated: bool,
}

/// `auto` detects new or patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PromotionChoice {
    #[default]
    Auto,
    New,
    Patch,
}

impl PromotionChoice {
    pub const fn kind(self) -> Option<PromotionKind> {
        match self {
            Self::Auto => None,
            Self::New => Some(PromotionKind::New),
            Self::Patch => Some(PromotionKind::Patch),
        }
    }
}

/// `POST /promotions`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct PromotionCreate {
    pub branch: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub kind: Option<PromotionChoice>,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub dry_run: Option<bool>,
    /// The preview's `main_before`: executing refuses if main moved since.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub expect_main: Option<String>,
    /// Promoting an older release than main has (a rollback) was shown and accepted.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub accept_rollback: Option<bool>,
    /// The commits a new promotion drops from main were shown and accepted.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub accept_losses: Option<bool>,
}

/// `POST /announcements`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct AnnouncementSend {
    pub analysis_id: String,
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub branch: Option<String>,
    pub recipients: Vec<String>,
}

/// The answer to `POST /announcements`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AnnouncementSent {
    pub delivery: Delivery,
    pub announcement: Announcement,
}

/// `POST /session`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct SignIn {
    /// A personal GitLab access token (scopes: read_api, write_repository).
    pub token: String,
}

/// An error body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ApiError {
    pub code: String,
    pub error: String,
}
