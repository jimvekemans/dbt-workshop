//! The TypeScript twins of the VaultSpeed API types, for the frontend's one `types.ts`: the hub's own
//! generator ([`vdp_release_hub::typescript_with`]) appends them to its types.

use vdp_release_hub::declaration;

use crate::api::{
    VsCommitCreate, VsFile, VsJobResult, VsMergeCreate, VsReleaseCreate, VsResetCreate, VsRun,
    VsSummary,
};

/// Every VaultSpeed type as `export type ...`, in a stable order.
pub fn declarations() -> Vec<String> {
    vec![
        declaration::<vdp_vaultspeed_release::zips::ZipInfo>(),
        declaration::<vdp_vaultspeed_release::status::CommitRef>(),
        declaration::<vdp_vaultspeed_release::status::EnvironmentState>(),
        declaration::<vdp_vaultspeed_release::status::ZipEnvironment>(),
        declaration::<vdp_vaultspeed_release::status::TicketSummary>(),
        declaration::<vdp_vaultspeed_release::status::ZipRow>(),
        declaration::<vdp_vaultspeed_release::status::ReleaseState>(),
        declaration::<vdp_vaultspeed_release::status::Status>(),
        declaration::<vdp_vaultspeed_release::flows::MergeStep>(),
        declaration::<vdp_vaultspeed_release::flows::PmlChange>(),
        declaration::<vdp_vaultspeed_release::flows::BuildRequest>(),
        declaration::<vdp_vaultspeed_release::flows::BuildOutcome>(),
        declaration::<vdp_vaultspeed_release::flows::MergeRequest>(),
        declaration::<vdp_vaultspeed_release::flows::LowerWarning>(),
        declaration::<vdp_vaultspeed_release::flows::MergeOutcome>(),
        declaration::<vdp_vaultspeed_release::flows::FileEdit>(),
        declaration::<vdp_vaultspeed_release::flows::CommitRequest>(),
        declaration::<vdp_vaultspeed_release::flows::CommitOutcome>(),
        declaration::<vdp_vaultspeed_release::flows::ResetRequest>(),
        declaration::<vdp_vaultspeed_release::flows::ResetOutcome>(),
        declaration::<vdp_vaultspeed_release::preview::SourcePreview>(),
        declaration::<vdp_vaultspeed_release::preview::PlanPreview>(),
        declaration::<VsSummary>(),
        declaration::<VsRun>(),
        declaration::<VsReleaseCreate>(),
        declaration::<VsMergeCreate>(),
        declaration::<VsCommitCreate>(),
        declaration::<VsResetCreate>(),
        declaration::<VsFile>(),
        declaration::<VsJobResult>(),
    ]
}

#[cfg(test)]
mod tests {
    /// The whole module: the hub's types and these. `VDP_TS_OUT=<file>` writes it (the frontend's `types.ts`).
    #[test]
    fn export_bindings() {
        let ts = vdp_release_hub::typescript_with(&super::declarations());
        assert!(ts.contains("export type VsSummary = "), "{ts}");
        assert!(ts.contains("export type Analysis = "), "{ts}");
        assert!(!ts.contains("bigint"), "{ts}");
        if let Some(out) = std::env::var_os("VDP_TS_OUT") {
            std::fs::write(&out, &ts).unwrap();
        }
    }
}
