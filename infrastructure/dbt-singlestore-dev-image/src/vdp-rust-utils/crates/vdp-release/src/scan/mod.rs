//! Tickets in commit history.
//!
//! - The **source** (`dev`) is scanned along its mainline (first parent), oldest first. Every mainline merge
//!   is classified by its message and by the commits it brings: a ticket merge, a hotfix/bugfix branch, a
//!   ticketless branch merge (a `branch:<name>` pseudo-ticket), a GitLab revert MR, a back-merge of a release
//!   branch or main, or a sync merge of the source into itself.
//! - `dev` must never receive a revert. Any revert or re-apply found on it (directly, through a revert MR, or
//!   brought by a back-merge) is a [`RawViolation`]; it still updates the content state, so the release is
//!   built from what `dev` really contains.
//! - The **compare** branch (main or a release branch) legitimately holds reverts and re-enables (reverts of
//!   reverts); [`compare_state`] gives each ticket's *net* state there.

mod graph;
mod parser;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub use graph::{SourceScanInput, compare_state, merged_tickets, scan_source};
pub(crate) use parser::{merge_branch_name, revert_depth, ticket_ids};

/// What kind of change a merge brought.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TicketKind {
    /// A Jira ticket (`INTEL-1234`).
    Jira,
    /// A `hotfix/<slug>` branch without a ticket: always kept.
    Hotfix,
    /// A `bugfix/<slug>` branch without a ticket: always kept.
    Bugfix,
    /// A merged branch without a ticket or hotfix/bugfix name (`branch:<name>`): kept by default, flagged.
    Branch,
}

/// A ticket brought in by a commit on the release's source branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedTicket {
    /// `INTEL-1234`, `hotfix/<slug>`, `bugfix/<slug>` or `branch:<name>`.
    pub ticket: String,
    pub kind: TicketKind,
    pub commit: String,
    pub author: String,
    pub timestamp: i64,
    /// Position of the mainline commit that brought it (0 = oldest).
    pub position: usize,
}

/// How a revert reached `dev`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "via", rename_all = "snake_case")]
pub enum ViolationVia {
    /// Committed on the mainline itself.
    Direct,
    /// Merged from a revert branch (GitLab's "Revert" button: `revert-<sha>`).
    RevertMerge { merge: String },
    /// Brought by merging a release branch or main into the source.
    BackMerge { merge: String, from: String },
}

/// A revert (or re-apply) found on the source branch, which must never have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawViolation {
    pub commit: String,
    pub tickets: Vec<String>,
    /// A re-apply (revert of a revert) rather than a revert.
    pub reapply: bool,
    pub via: ViolationVia,
    /// Position of the mainline commit through which it arrived.
    pub position: usize,
}

/// What the source mainline holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceScan {
    /// Ticket merges still in effect, in mainline order.
    pub merged: Vec<MergedTicket>,
    pub violations: Vec<RawViolation>,
    /// Other ticket ids named by a ticket's merges or the commits they bring.
    pub also_mentions: BTreeMap<String, BTreeSet<String>>,
    /// Positions of direct mainline commits without a ticket.
    pub uncoupled: Vec<usize>,
}

/// Branch names the scan treats specially.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanNames {
    /// The source branch (`dev`): merging it into itself is a sync merge.
    pub source: String,
    /// `main`: merging it into the source is a back-merge.
    pub main: String,
    /// `release/`: merging such a branch into the source is a back-merge.
    pub release_prefix: String,
}

/// Each ticket's net state on the compare branch since the merge-base.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompareState {
    /// Tickets whose latest change there is a revert: the commit that reverted them.
    pub reverted: BTreeMap<String, String>,
    /// Tickets whose latest change there re-enabled them (a revert of a revert): that commit.
    pub reenabled: BTreeMap<String, String>,
    /// Tickets named by a non-revert commit there (merged, cherry-picked or committed on it).
    pub present: BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use vdp_git::Commit;

    use super::*;

    fn commit(hash: &str, message: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: hash.into(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            author: "a".into(),
            timestamp: 0,
            message: message.into(),
        }
    }

    /// A merge on the mainline, its first parent the previous mainline commit.
    fn merge(hash: &str, message: &str, prev: &str, side: &str) -> Commit {
        commit(hash, message, &[prev, side])
    }

    fn names() -> ScanNames {
        ScanNames {
            source: "dev".into(),
            main: "main".into(),
            release_prefix: "release/".into(),
        }
    }

    fn ids(scan: &SourceScan) -> Vec<(&str, &str)> {
        scan.merged
            .iter()
            .map(|t| (t.ticket.as_str(), t.commit.as_str()))
            .collect()
    }

    #[test]
    fn ticket_ids_are_whole_and_untruncated() {
        assert_eq!(
            ticket_ids(
                "Merge branch 'feature/intel-123456-x' fixes INTEL-7, XINTEL-9",
                "INTEL"
            ),
            ["INTEL-123456", "INTEL-7"]
        );
    }

    #[test]
    fn revert_depth_from_subject_and_trailer() {
        let known = HashMap::new();
        let depth = |m: &str| revert_depth(&commit("x", m, &["p"]), &known);
        assert_eq!(depth("Merge branch 'feature/INTEL-1'"), 0);
        assert_eq!(depth("fix: revert reference table for INTEL-3"), 0);
        assert_eq!(depth("Merge branch 'revert-4b89f7f6' into 'dev'"), 0);
        assert_eq!(depth("Revert \"Merge branch 'feature/INTEL-1'\""), 1);
        assert_eq!(
            depth("Revert \"Revert \"Merge branch 'feature/INTEL-1'\"\""),
            2
        );
        assert_eq!(depth("Reapply \"Merge branch 'feature/INTEL-1'\""), 2);
        assert_eq!(
            depth("Revert \"Reapply \"Merge branch 'feature/INTEL-1'\"\""),
            3,
            "a revert of a re-apply reverts again"
        );
        let original = commit(
            "aaaaaaa1",
            "Undo INTEL-1\n\nThis reverts commit bbbbbbb2.",
            &["p"],
        );
        let again = commit(
            "ccccccc3",
            "Back again\n\nThis reverts commit aaaaaaa1.",
            &["p"],
        );
        let known: HashMap<&str, &Commit> =
            [(original.hash.as_str(), &original)].into_iter().collect();
        assert_eq!(revert_depth(&original, &known), 1);
        assert_eq!(revert_depth(&again, &known), 2);
    }

    #[test]
    fn hotfix_bugfix_and_ticketless_branches_are_pseudo_tickets() {
        let mainline = [
            commit("r", "initial", &[]),
            merge(
                "h1",
                "Merge branch 'hotfix/fix-payment.timeout' into 'dev'",
                "r",
                "x1",
            ),
            merge("b1", "Merge branch 'BUGFIX/x_y' into 'dev'", "h1", "x2"),
            merge(
                "f1",
                "Merge branch 'hotfix/INTEL-5-thing' into 'dev'",
                "b1",
                "x3",
            ),
            merge("g1", "Merge branch 'fusion' into 'dev'", "f1", "x4"),
        ];
        let found: Vec<(String, TicketKind)> = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &mainline,
            prefix: "INTEL",
            names: &names(),
        })
        .merged
        .into_iter()
        .map(|t| (t.ticket, t.kind))
        .collect();
        assert_eq!(
            found,
            [
                ("hotfix/fix-payment.timeout".into(), TicketKind::Hotfix),
                ("bugfix/x_y".into(), TicketKind::Bugfix),
                ("INTEL-5".into(), TicketKind::Jira),
                ("branch:fusion".into(), TicketKind::Branch),
            ]
        );
    }

    #[test]
    fn a_revert_on_dev_is_a_violation_and_cancels_the_merge() {
        let mainline = [
            commit("r", "initial", &[]),
            merge("m1", "Merge branch 'feature/INTEL-1' into 'dev'", "r", "f1"),
            merge(
                "m2",
                "Merge branch 'feature/INTEL-2' into 'dev'",
                "m1",
                "f2",
            ),
            commit(
                "v1",
                "Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"\n\nThis reverts commit m1, reversing\nchanges made to r.",
                &["m2"],
            ),
            merge(
                "m3",
                "Merge branch 'feature/INTEL-1' into 'dev'",
                "v1",
                "f3",
            ),
        ];
        let scan = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &mainline,
            prefix: "INTEL",
            names: &names(),
        });
        assert_eq!(ids(&scan), [("INTEL-2", "m2"), ("INTEL-1", "m3")]);
        assert_eq!(scan.violations.len(), 1);
        assert_eq!(scan.violations[0].commit, "v1");
        assert_eq!(scan.violations[0].via, ViolationVia::Direct);
        assert!(!scan.violations[0].reapply);
        assert!(
            scan.uncoupled.is_empty(),
            "a revert is not an uncoupled change"
        );
    }

    #[test]
    fn a_reapply_on_dev_brings_the_ticket_back_and_is_a_violation() {
        let mainline = [
            commit("r", "initial", &[]),
            merge("m1", "Merge branch 'feature/INTEL-1' into 'dev'", "r", "f1"),
            commit(
                "v1",
                "Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"",
                &["m1"],
            ),
            commit(
                "v2",
                "Revert \"Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"\"",
                &["v1"],
            ),
        ];
        let scan = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &mainline,
            prefix: "INTEL",
            names: &names(),
        });
        assert_eq!(ids(&scan), [("INTEL-1", "v2")]);
        assert_eq!(scan.violations.len(), 2);
        assert!(scan.violations[1].reapply);
    }

    /// GitLab's "Revert" button: a `revert-<sha>` branch holding the revert, merged into dev (f12479fe).
    #[test]
    fn a_gitlab_revert_merge_is_a_revert_not_a_second_merge() {
        let feature = commit("f1", "INTEL-24954 work", &["r"]);
        let revert = commit(
            "rv",
            "Revert \"Merge branch 'feature/INTEL-24954' into 'dev'\"\n\nThis reverts commit m1, reversing\nchanges made to r.",
            &["m1"],
        );
        let mainline = [
            commit("r", "initial", &[]),
            merge(
                "m1",
                "Merge branch 'feature/INTEL-24954' into 'dev'",
                "r",
                "f1",
            ),
            merge(
                "m2",
                "Merge branch 'revert-4b89f7f6' into 'dev'\n\nRevert \"Merge branch 'feature/INTEL-24954' into 'dev'\"\n\nSee merge request vdp/dbt-vdp!168",
                "m1",
                "rv",
            ),
        ];
        let all: Vec<Commit> = [
            mainline[0].clone(),
            feature,
            mainline[1].clone(),
            revert,
            mainline[2].clone(),
        ]
        .into();
        let scan = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &all,
            prefix: "INTEL",
            names: &names(),
        });
        assert!(scan.merged.is_empty(), "{:?}", scan.merged);
        assert_eq!(scan.violations.len(), 1);
        assert_eq!(
            scan.violations[0].via,
            ViolationVia::RevertMerge { merge: "m2".into() }
        );
    }

    /// A release branch merged back into dev brings its reverts along (365e5322).
    #[test]
    fn a_back_merge_brings_reverts_and_is_a_violation() {
        let revert = commit(
            "rv",
            "Revert \"Merge branch 'feature/INTEL-2' into 'dev'\"",
            &["m2"],
        );
        let mainline = [
            commit("r", "initial", &[]),
            merge("m2", "Merge branch 'feature/INTEL-2' into 'dev'", "r", "f2"),
            merge("bm", "Merge branch 'release/2026_DROP04'", "m2", "rv"),
        ];
        let all: Vec<Commit> = [
            mainline[0].clone(),
            mainline[1].clone(),
            revert,
            mainline[2].clone(),
        ]
        .into();
        let scan = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &all,
            prefix: "INTEL",
            names: &names(),
        });
        assert_eq!(scan.merged, []);
        assert_eq!(
            scan.violations[0].via,
            ViolationVia::BackMerge {
                merge: "bm".into(),
                from: "release/2026_DROP04".into()
            }
        );
    }

    #[test]
    fn a_sync_merge_of_dev_scans_what_it_brings() {
        let inner = merge("im", "Merge branch 'feature/INTEL-9' into 'dev'", "x", "f9");
        let mainline = [
            commit("r", "initial", &[]),
            merge("s", "Merge branch 'dev' from dbt-swf into dev", "r", "im"),
        ];
        let all: Vec<Commit> = [mainline[0].clone(), inner, mainline[1].clone()].into();
        let scan = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &all,
            prefix: "INTEL",
            names: &names(),
        });
        assert_eq!(ids(&scan), [("INTEL-9", "im")]);
    }

    #[test]
    fn other_ids_in_a_merge_are_also_mentions() {
        let inner = commit("c1", "Feat / INTEL-27178 dubbele rijen", &["r"]);
        let mainline = [
            commit("r", "initial", &[]),
            merge(
                "m",
                "Merge branch 'feature/INTEL-27128' into 'dev'\n\nCloses INTEL-27128",
                "r",
                "c1",
            ),
        ];
        let all: Vec<Commit> = [mainline[0].clone(), inner, mainline[1].clone()].into();
        let scan = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &all,
            prefix: "INTEL",
            names: &names(),
        });
        assert_eq!(ids(&scan), [("INTEL-27128", "m")]);
        assert_eq!(
            scan.also_mentions["INTEL-27128"].iter().collect::<Vec<_>>(),
            ["INTEL-27178"]
        );
    }

    #[test]
    fn uncoupled_commits_are_direct_changes() {
        let mainline = [
            commit("r", "initial", &[]),
            commit("d1", "fix typo in README", &["r"]),
            commit("d2", "INTEL-3 quick fix", &["d1"]),
            commit("d3", "fix: revert reference table", &["d2"]),
        ];
        let scan = scan_source(SourceScanInput {
            mainline: &mainline,
            all: &mainline,
            prefix: "INTEL",
            names: &names(),
        });
        assert_eq!(scan.uncoupled, [1, 3]);
        assert_eq!(ids(&scan), [("INTEL-3", "d2")]);
    }

    /// The patch tool re-enables with a revert of the revert; that ticket is not "reverted" any more (it was,
    /// and a second patch reverted the re-enable: the feature was removed again).
    #[test]
    fn compare_state_follows_reverts_and_re_enables() {
        let commits = [
            commit(
                "a",
                "Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"",
                &["p"],
            ),
            commit(
                "b",
                "Revert \"Merge branch 'feature/INTEL-2' into 'dev'\"",
                &["a"],
            ),
            commit(
                "c",
                "Revert \"Revert \"Merge branch 'feature/INTEL-1' into 'dev'\"\"",
                &["b"],
            ),
            commit(
                "d",
                "feature/INTEL-27232: fix on the release branch",
                &["c"],
            ),
            commit(
                "e",
                "Merge branch 'feature/INTEL-5-revert-old-logic'",
                &["d", "x"],
            ),
            commit("f", "Revert INTEL-6 and INTEL-7", &["e"]),
        ];
        let state = compare_state(&commits, "INTEL");
        assert_eq!(
            state.reverted.keys().collect::<Vec<_>>(),
            ["INTEL-2", "INTEL-6", "INTEL-7"]
        );
        assert_eq!(state.reenabled["INTEL-1"], "c");
        assert!(state.present.contains("INTEL-27232"));
        assert!(
            state.present.contains("INTEL-5"),
            "a branch named revert-… is not a revert"
        );
    }
}
