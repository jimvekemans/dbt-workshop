//! The request and response bodies of `/api/v1/vaultspeed/...`. Their TypeScript twins come with the `ts` feature.

use serde::{Deserialize, Serialize};
use vdp_release::{OnConflictChoice, StrategyChoice};
use vdp_release_hub::api::RepoStatus;
use vdp_release_hub::jobs::Job;

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

/// What a finished VaultSpeed job produced. It travels in the hub's job list as the job's `result`, so the
/// `type` tags are the hub's own `vs_*` names for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VsJobResult {
    #[serde(rename = "vs_release")]
    Release {
        outcome: vdp_vaultspeed_release::flows::BuildOutcome,
    },
    #[serde(rename = "vs_merge")]
    Merge {
        outcome: vdp_vaultspeed_release::flows::MergeOutcome,
        plan: Option<vdp_vaultspeed_release::preview::PlanPreview>,
    },
    #[serde(rename = "vs_commit")]
    Commit {
        outcome: vdp_vaultspeed_release::flows::CommitOutcome,
    },
    #[serde(rename = "vs_reset")]
    Reset {
        outcome: vdp_vaultspeed_release::flows::ResetOutcome,
    },
}

impl VsJobResult {
    /// As the hub's job result. Plain data cannot fail to serialise; should it, the job still ends, with the
    /// reason as its result instead of a lost one.
    pub fn into_result(self) -> vdp_release_hub::jobs::JobResult {
        vdp_release_hub::jobs::JobResult::Extension(serde_json::to_value(&self).unwrap_or_else(
            |e| serde_json::json!({ "type": "vs_error", "message": e.to_string() }),
        ))
    }
}
