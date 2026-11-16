//! The dbt DROP release flows, shared by `vdp release ...` and the release hub.
//!
//! - [`Analyser`] builds an [`Analysis`]: the tickets merged on the source mainline since the merge-base with
//!   the compare branch (oldest first, a later revert cancels a merge, hotfix/bugfix branches as pseudo-tickets),
//!   the reverts on the compare branch, direct ("uncoupled") commits, Jira data per ticket, and the
//!   [`ReleasePolicy`] classification with a default selection.
//! - [`ReleaseFlow`] cuts a release branch with a selection of tickets reverted (newest first) and the compare
//!   branch's reverts of out-of-scope tickets carried over; [`PatchFlow`] adds tickets to a release branch
//!   (re-enabling reverted ones, cherry-picking merges with `-m 1`); [`Promotion`] brings a release to main.
//! - Every flow refuses an analysis whose refs moved, refuses a dirty work tree, and changes nothing in a dry run.

mod analysis;
mod branches;
mod changelog;
pub mod changes;
mod conflicts;
mod error;
mod flow;
mod origin;
mod patch;
mod policy;
mod promote;
mod rules;
mod scan;
mod settings;

pub use analysis::{
    Analyser, Analysis, AnalysisKind, AnalysisRequest, CommitInfo, CompareRevert, DevViolation,
    JiraInfo, Progress, TicketEntry,
};
pub use branches::{ReleaseBranch, release_branches};
pub use changelog::{Changelog, ChangelogEntry, ChangelogPatch};
pub use conflicts::{
    ConflictFile, ConflictReport, Conflicts, FileChoice, OnConflictChoice, Resolution,
    ResolutionAction, StrategyChoice, SuggestedAction, Suggestion, apply_options, plain_report,
};
pub use error::ReleaseError;
pub use flow::{
    ChangelogFile, Outcome, ReleaseFlow, ReleaseOptions, Step, StepOutcome, release_steps,
};
pub use origin::{
    BranchRole, CHANGELOG_TRAILER, OriginState, ReleaseOnly, ScopeAction, check_scope,
    origin_state, release_only,
};
pub use patch::{PatchFlow, patch_steps};
pub use policy::{Classification, Decision, ReleasePolicy};
pub use promote::{Detection, PromoteOptions, Promoted, Promotion, PromotionKind, detect};
pub use rules::BranchRules;
pub use scan::{
    CompareState, MergedTicket, RawViolation, ScanNames, SourceScan, TicketKind, ViolationVia,
    compare_state, merged_tickets, scan_source,
};
pub use settings::ReleaseSettings;

/// `dev` → `origin/dev`; `origin/...` and commit hashes stay as they are (`_to_ref` in `release_service.py`).
pub fn remote_ref(name: &str) -> String {
    let name = name.trim();
    let is_hash = (7..=40).contains(&name.len()) && name.chars().all(|c| c.is_ascii_hexdigit());
    if name.starts_with("origin/") || is_hash {
        name.to_owned()
    } else {
        format!("origin/{name}")
    }
}

/// `origin/release/x` → `release/x`; a duplicated `release/release/` prefix is collapsed
/// (`_sanitize_branch_name`).
pub fn local_name(name: &str) -> String {
    let name = name.trim();
    let mut name = name.strip_prefix("origin/").unwrap_or(name).to_owned();
    while name.starts_with("release/release/") {
        name = name["release/".len()..].to_owned();
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_names() {
        assert_eq!(remote_ref("dev"), "origin/dev");
        assert_eq!(remote_ref("origin/main"), "origin/main");
        assert_eq!(remote_ref("40ae48711c65"), "40ae48711c65");
        assert_eq!(
            remote_ref("release/2026_DROP24"),
            "origin/release/2026_DROP24"
        );
        assert_eq!(local_name("origin/release/release/x"), "release/x");
    }
}
