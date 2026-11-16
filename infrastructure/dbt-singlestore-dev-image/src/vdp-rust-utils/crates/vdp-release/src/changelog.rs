//! The release changelog: `changelog/<fix version>.md`, the last commit of a release branch.
//!
//! Three sections: features reverted by an earlier release and now accepted, features added, features
//! excluded. A patch drops the changelog commit, applies its steps, and commits a rewritten changelog on top.
//! The front matter is JSON (valid YAML) so the next patch can read it back exactly.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::analysis::{Analysis, TicketEntry};
use crate::flow::{Step, StepOutcome};
use crate::policy::Classification;

/// One ticket line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangelogEntry {
    pub id: String,
    pub summary: String,
    pub fix_versions: Vec<String>,
    pub reason: String,
}

/// A patch of the release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ChangelogPatch {
    pub date: String,
    pub by: String,
    /// Re-enabled (previously reverted, now accepted).
    pub accepted: Vec<String>,
    /// Added (cherry-picked).
    pub added: Vec<String>,
}

/// The changelog's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Changelog {
    pub version: u32,
    pub release: String,
    pub branch: String,
    pub source_head: String,
    pub date: String,
    pub by: String,
    pub accepted: Vec<ChangelogEntry>,
    pub added: Vec<ChangelogEntry>,
    pub excluded: Vec<ChangelogEntry>,
    pub patches: Vec<ChangelogPatch>,
}

/// The path of a release's changelog.
pub(crate) fn path(fix_version: &str) -> String {
    format!("changelog/{fix_version}.md")
}

/// The changelog commit message: no ticket ids (the scans must not take it for a ticket change).
pub(crate) fn commit_message(fix_version: &str) -> String {
    format!(
        "Changelog {fix_version}\n\n{} {fix_version}",
        crate::CHANGELOG_TRAILER
    )
}

fn entry(ticket: &TicketEntry) -> ChangelogEntry {
    let summary = ticket
        .jira
        .as_ref()
        .map(|j| j.summary.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| ticket.merges.last().map(|m| m.subject.clone()))
        .or_else(|| {
            ticket
                .reverted_on_compare
                .as_ref()
                .map(|c| c.subject.clone())
        })
        .unwrap_or_default();
    ChangelogEntry {
        id: ticket.id.clone(),
        summary,
        fix_versions: ticket
            .jira
            .as_ref()
            .map(|j| j.fix_versions.clone())
            .unwrap_or_default(),
        reason: match &ticket.classification {
            Classification::Keep(reason) | Classification::Revert(reason) => reason.clone(),
            Classification::NoFixVersion => "no DROP fix version".into(),
        },
    }
}

/// Whether a step really took effect (a skipped step leaves the ticket as it was).
fn took_effect(outcome: StepOutcome) -> bool {
    matches!(
        outcome,
        StepOutcome::Applied | StepOutcome::Resolved | StepOutcome::Empty | StepOutcome::Planned
    )
}

/// A release's changelog, from what its steps actually did.
pub(crate) fn for_release(
    analysis: &Analysis,
    branch: &str,
    steps: &[(Step, StepOutcome)],
    by: &str,
    date: &str,
) -> Changelog {
    let excluded: BTreeSet<&str> = steps
        .iter()
        .filter(|(_, outcome)| took_effect(*outcome))
        .flat_map(|(step, _)| match step {
            Step::Revert { .. } | Step::CherryPick { .. } => step.tickets(),
            Step::ReEnable { .. } | Step::Pick { .. } => Vec::new(),
        })
        .collect();
    let mut log = Changelog {
        version: 1,
        release: analysis.request.fix_version.clone(),
        branch: branch.to_owned(),
        source_head: analysis.source_head.clone(),
        date: date.to_owned(),
        by: by.to_owned(),
        accepted: Vec::new(),
        added: Vec::new(),
        excluded: Vec::new(),
        patches: Vec::new(),
    };
    for ticket in &analysis.tickets {
        let id = ticket.id.as_str();
        if excluded.contains(id) {
            log.excluded.push(entry(ticket));
        } else if !ticket.merges.is_empty() {
            log.added.push(entry(ticket));
        } else if ticket.reverted_on_compare.is_some() {
            // Reverted by an earlier release; its revert is not carried over: accepted now.
            log.accepted.push(entry(ticket));
        }
    }
    log
}

/// A patch: re-enabled tickets move from excluded to accepted, picked ones are added.
pub(crate) fn for_patch(
    previous: Option<Changelog>,
    analysis: &Analysis,
    branch: &str,
    steps: &[(Step, StepOutcome)],
    by: &str,
    date: &str,
) -> Changelog {
    let mut log = previous.unwrap_or_else(|| {
        // A release cut before changelogs existed: start from what the branch has.
        let mut fresh = for_release(analysis, branch, &[], by, date);
        fresh.added.clear();
        fresh.accepted.clear();
        fresh.excluded = analysis
            .tickets
            .iter()
            .filter(|t| t.reverted_on_compare.is_some())
            .map(entry)
            .collect();
        fresh
    });
    let mut patch = ChangelogPatch {
        date: date.to_owned(),
        by: by.to_owned(),
        accepted: Vec::new(),
        added: Vec::new(),
    };
    for (step, outcome) in steps {
        if !took_effect(*outcome) {
            continue;
        }
        match step {
            Step::ReEnable { ticket, .. } => {
                let found = log.excluded.iter().position(|e| &e.id == ticket);
                let moved = match found {
                    Some(index) => log.excluded.remove(index),
                    None => analysis.ticket(ticket).map_or_else(
                        || ChangelogEntry {
                            id: ticket.clone(),
                            summary: String::new(),
                            fix_versions: Vec::new(),
                            reason: String::new(),
                        },
                        entry,
                    ),
                };
                if !log.accepted.iter().any(|e| &e.id == ticket) {
                    log.accepted.push(moved);
                }
                patch.accepted.push(ticket.clone());
            }
            Step::Pick { ticket, .. } => {
                if !log.added.iter().any(|e| &e.id == ticket)
                    && let Some(found) = analysis.ticket(ticket)
                {
                    log.added.push(entry(found));
                }
                if !patch.added.contains(ticket) {
                    patch.added.push(ticket.clone());
                }
            }
            Step::Revert { .. } | Step::CherryPick { .. } => {}
        }
    }
    log.patches.push(patch);
    log
}

impl Changelog {
    /// The file: JSON front matter (valid YAML), then Markdown.
    pub fn render(&self) -> String {
        let front = serde_json::to_string_pretty(&serde_json::json!({ "vdp_changelog": self }))
            .unwrap_or_default();
        let mut out = format!(
            "---\n{front}\n---\n\n# Release {}\n\nBranch `{}`, cut from `{}` on {} by {}.\n",
            self.release,
            self.branch,
            &self.source_head[..self.source_head.len().min(8)],
            self.date,
            self.by
        );
        let section = |out: &mut String, title: &str, entries: &[ChangelogEntry]| {
            let _ = write!(out, "\n## {title} ({})\n\n", entries.len());
            if entries.is_empty() {
                out.push_str("None.\n");
                return;
            }
            out.push_str("| Ticket | Summary | Fix version | Why |\n|---|---|---|---|\n");
            for e in entries {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} |",
                    e.id,
                    e.summary.replace('|', "\\|"),
                    e.fix_versions.join(", "),
                    e.reason.replace('|', "\\|")
                );
            }
        };
        section(
            &mut out,
            "Previously reverted, now accepted",
            &self.accepted,
        );
        section(&mut out, "Added", &self.added);
        section(&mut out, "Excluded", &self.excluded);
        if !self.patches.is_empty() {
            out.push_str("\n## Patches\n\n");
            for p in &self.patches {
                let list = |ids: &[String]| {
                    if ids.is_empty() {
                        "none".to_owned()
                    } else {
                        ids.join(", ")
                    }
                };
                let _ = writeln!(
                    out,
                    "- {} by {}: accepted {}; added {}",
                    p.date,
                    p.by,
                    list(&p.accepted),
                    list(&p.added)
                );
            }
        }
        out
    }

    /// Reads a file written by [`Changelog::render`].
    pub fn parse(text: &str) -> Option<Self> {
        let rest = text.strip_prefix("---\n")?;
        let end = rest.find("\n---\n")?;
        let value: serde_json::Value = serde_json::from_str(&rest[..end]).ok()?;
        serde_json::from_value(value.get("vdp_changelog")?.clone()).ok()
    }
}

/// Today (UTC) as `YYYY-MM-DD`.
pub(crate) fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400);
    let days = i64::try_from(days).unwrap_or(0);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_and_parse_round_trip() {
        let log = Changelog {
            version: 1,
            release: "2026_DROP25".into(),
            branch: "release/2026_DROP25".into(),
            source_head: "7edfd39e462eb61a".into(),
            date: "2026-11-09".into(),
            by: "Release Manager".into(),
            accepted: vec![],
            added: vec![ChangelogEntry {
                id: "INTEL-1".into(),
                summary: "Load | contacts".into(),
                fix_versions: vec!["2026_DROP25".into()],
                reason: "2026_DROP25".into(),
            }],
            excluded: vec![],
            patches: vec![],
        };
        let text = log.render();
        assert!(text.starts_with("---\n{"));
        assert!(text.contains("| INTEL-1 | Load \\| contacts |"));
        assert_eq!(Changelog::parse(&text), Some(log));
        assert!(!commit_message("2026_DROP25").contains("INTEL"));
    }

    #[test]
    fn today_is_a_date() {
        let today = today();
        assert_eq!(today.len(), 10);
        assert!(today.starts_with("20"));
    }
}
