//! The API's TypeScript types in one module, for the frontend (`frontend/src/lib/api/types.ts`).

use ts_rs::{Config, TS};

use crate::api::{
    AnalysisCreate, AnalysisSummary, AnalysisView, AnnouncementSend, AnnouncementSent, ApiError,
    CloneRequest, ConflictVersionsView, Defaults, GitIdentity, HealthProblem, PatchCreate,
    PromotionChoice, PromotionCreate, Readiness, ReleaseCreate, RepoStatus, SelectionUpdate,
    SignIn, Status, VsCommitCreate, VsFile, VsMergeCreate, VsReleaseCreate, VsResetCreate, VsRun,
    VsSummary,
};
use crate::jobs::{Job, JobKind, JobResult, JobStatus, JobStep, Level};

macro_rules! declarations {
    ($cfg:expr; $($ty:ty),* $(,)?) => {
        vec![$(format!("export {}", <$ty as TS>::decl($cfg))),*]
    };
}

/// Every API type as `export type ...`, in a stable order.
#[allow(clippy::too_many_lines, reason = "one list of types")]
pub fn typescript() -> String {
    let cfg = Config::new().with_large_int("number");
    let cfg = &cfg;
    let decls = declarations![cfg;
        vdp_release::AnalysisKind,
        vdp_release::AnalysisRequest,
        vdp_release::CommitInfo,
        vdp_release::JiraInfo,
        vdp_release::TicketKind,
        vdp_release::Classification,
        vdp_release::TicketEntry,
        vdp_release::CompareRevert,
        vdp_release::Analysis,
        vdp_release::Step,
        vdp_release::StepOutcome,
        vdp_release::Outcome,
        vdp_release::PromotionKind,
        vdp_release::Detection,
        vdp_release::Promoted,
        vdp_release::ReleaseBranch,
        vdp_release::BranchRole,
        vdp_release::OriginState,
        vdp_release::ReleaseOnly,
        vdp_release::ViolationVia,
        vdp_release::DevViolation,
        vdp_release::ChangelogFile,
        vdp_release::ConflictFile,
        vdp_release::SuggestedAction,
        vdp_release::Suggestion,
        vdp_release::ConflictReport,
        vdp_release::FileChoice,
        vdp_release::ResolutionAction,
        vdp_release::Resolution,
        vdp_release::OnConflictChoice,
        vdp_release::StrategyChoice,
        vdp_release::changes::ChangedFile,
        vdp_release::changes::FileChanges,
        vdp_release::changes::ArchiveEntry,
        vdp_release::changes::FileDiff,
        vdp_notify::AnnouncementRow,
        vdp_notify::Announcement,
        vdp_notify::Delivery,
        vdp_vaultspeed_release::zips::ZipInfo,
        vdp_vaultspeed_release::status::CommitRef,
        vdp_vaultspeed_release::status::EnvironmentState,
        vdp_vaultspeed_release::status::ZipEnvironment,
        vdp_vaultspeed_release::status::TicketSummary,
        vdp_vaultspeed_release::status::ZipRow,
        vdp_vaultspeed_release::status::ReleaseState,
        vdp_vaultspeed_release::status::Status,
        vdp_vaultspeed_release::flows::MergeStep,
        vdp_vaultspeed_release::flows::PmlChange,
        vdp_vaultspeed_release::flows::BuildRequest,
        vdp_vaultspeed_release::flows::BuildOutcome,
        vdp_vaultspeed_release::flows::MergeRequest,
        vdp_vaultspeed_release::flows::LowerWarning,
        vdp_vaultspeed_release::flows::MergeOutcome,
        vdp_vaultspeed_release::flows::FileEdit,
        vdp_vaultspeed_release::flows::CommitRequest,
        vdp_vaultspeed_release::flows::CommitOutcome,
        vdp_vaultspeed_release::flows::ResetRequest,
        vdp_vaultspeed_release::flows::ResetOutcome,
        vdp_vaultspeed_release::preview::SourcePreview,
        vdp_vaultspeed_release::preview::PlanPreview,
        RepoStatus,
        HealthProblem,
        crate::auth::BranchRight,
        crate::auth::Pipeline,
        crate::auth::SessionUser,
        SignIn,
        ConflictVersionsView,
        GitIdentity,
        Readiness,
        Defaults,
        VsSummary,
        Status,
        VsRun,
        VsReleaseCreate,
        VsMergeCreate,
        VsCommitCreate,
        VsResetCreate,
        VsFile,
        CloneRequest,
        AnalysisCreate,
        AnalysisView,
        AnalysisSummary,
        SelectionUpdate,
        ReleaseCreate,
        PatchCreate,
        PromotionChoice,
        PromotionCreate,
        AnnouncementSend,
        AnnouncementSent,
        ApiError,
        JobKind,
        JobStatus,
        Level,
        JobStep,
        JobResult,
        Job,
    ];
    let mut out = String::from(
        "// Generated from the Rust API types (vdp-release-hub, feature `ts`). Do not edit:\n\
         // VDP_TS_OUT=<this file> cargo test -p vdp-release-hub --features ts export_bindings\n\n",
    );
    for decl in decls {
        out.push_str(&decl);
        out.push_str("\n\n");
    }
    out.trim_end().to_owned() + "\n"
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_bindings() {
        let ts = super::typescript();
        assert!(ts.contains("export type Analysis = "), "{ts}");
        assert!(!ts.contains("bigint"), "{ts}");
        if let Some(out) = std::env::var_os("VDP_TS_OUT") {
            std::fs::write(&out, &ts).unwrap();
        }
    }
}
