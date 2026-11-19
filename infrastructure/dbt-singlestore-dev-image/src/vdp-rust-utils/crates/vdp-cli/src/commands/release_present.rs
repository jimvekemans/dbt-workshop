//! Terminal formatting and prompts for `vdp release` and `vdp jira`.

use std::io::{BufRead, Write};

use vdp_core::BatchOutcome;
use vdp_jira::{AdvanceRecord, TicketAdvance};
use vdp_release::{
    Analysis, AnalysisKind, BranchRules, Classification, Promoted, PromotionKind, ReleaseBranch,
    Step, ViolationVia, local_name,
};

/// Reverts on the source (dev), which must never have one: reported, never repeated.
fn print_violations(analysis: &Analysis) {
    if analysis.dev_violations.is_empty() {
        return;
    }
    tracing::warn!(
        "WARNING: {} has {} revert(s); it must never have one (the release accounts for them):",
        analysis.request.source,
        analysis.dev_violations.len()
    );
    for violation in &analysis.dev_violations {
        let via = match &violation.via {
            ViolationVia::Direct => "committed directly".to_owned(),
            ViolationVia::RevertMerge { merge } => {
                format!(
                    "merged from a revert branch ({})",
                    &merge[..merge.len().min(8)]
                )
            }
            ViolationVia::BackMerge { from, .. } => format!("brought by merging {from}"),
        };
        tracing::warn!(
            "  {} {} {} ({via})",
            violation.commit.short,
            if violation.reapply {
                "re-applies"
            } else {
                "reverts"
            },
            violation.tickets.join(", ")
        );
    }
}

pub(super) fn print_analysis(analysis: &Analysis) {
    let request = &analysis.request;
    println!(
        "{:?} {} vs {} for {} (merge-base {})",
        request.kind,
        request.source,
        request.compare,
        request.fix_version,
        &analysis.merge_base[..analysis.merge_base.len().min(12)]
    );
    for ticket in &analysis.tickets {
        let mark = match (request.kind, ticket.selected_by_default) {
            (AnalysisKind::Release, true) => "revert ",
            (AnalysisKind::Release, false) => "keep   ",
            (AnalysisKind::Patch, true) => "include",
            (AnalysisKind::Patch, false) => "       ",
        };
        let why = match &ticket.classification {
            Classification::Keep(reason) | Classification::Revert(reason) => reason.clone(),
            Classification::NoFixVersion => ticket
                .jira_error
                .clone()
                .unwrap_or_else(|| "no fix version".into()),
        };
        let mut flags = Vec::new();
        if ticket.reverted_on_compare.is_some() {
            flags.push("reverted on compare");
        }
        if ticket.already_on_compare {
            flags.push("already on compare");
        }
        println!(
            "{mark} {:<28} {} merge(s)  {why}{}",
            ticket.id,
            ticket.merges.len(),
            if flags.is_empty() {
                String::new()
            } else {
                format!("  [{}]", flags.join(", "))
            }
        );
    }
    print_violations(analysis);
    if !analysis.uncoupled.is_empty() {
        println!(
            "{} commit(s) on the source without a ticket:",
            analysis.uncoupled.len()
        );
        for commit in &analysis.uncoupled {
            println!(
                "        {} {} ({})",
                commit.short, commit.subject, commit.author
            );
        }
    }
}

pub(super) fn print_outcome(outcome: &vdp_release::Outcome) {
    for (step, state) in &outcome.steps {
        println!(
            "{:<10} {}",
            format!("{state:?}").to_lowercase(),
            step.describe()
        );
    }
    if outcome.pushed {
        println!("pushed {}", outcome.branch);
    } else if outcome.dry_run {
        println!(
            "DRY RUN: nothing changed; {} would be pushed",
            outcome.branch
        );
    }
}

pub(super) fn confirm(step: &Step, index: usize, total: usize) -> bool {
    print!(
        "[{index:02}/{total:02}] {}. Proceed? [Y/n] ",
        step.describe()
    );
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_lowercase().as_str(), "" | "y" | "yes")
}

pub(super) fn print_promotion(done: &Promoted, rules: &BranchRules, to: &str) {
    if done.nothing_to_promote {
        println!(
            "nothing to promote: {} is already on {}",
            done.branch, rules.main
        );
        return;
    }
    for lost in &done.losses {
        println!(
            "drops from {}: {} {} (not on {})",
            rules.main, lost.commit.short, lost.commit.subject, rules.source
        );
    }
    println!(
        "{} {} to {} ({:?}: {}){}",
        if done.pushed {
            "promoted"
        } else {
            "would promote"
        },
        done.branch,
        local_name(to),
        done.kind.unwrap_or(PromotionKind::New),
        done.reason,
        done.main_head
            .as_deref()
            .map(|h| format!(", main is now {}", &h[..h.len().min(12)]))
            .unwrap_or_default()
    );
}

pub(super) fn print_branches(branches: &[ReleaseBranch]) {
    for branch in branches {
        println!(
            "{:<28} {} {}",
            branch.name,
            &branch.head[..branch.head.len().min(10)],
            if branch.promoted { "on main" } else { "" }
        );
    }
}

pub(super) fn format_jira_advance_line(record: &AdvanceRecord) -> Option<String> {
    let key = &record.key;
    match &record.outcome {
        BatchOutcome::Succeeded(TicketAdvance::Advanced {
            before,
            after,
            transition,
        }) => Some(format!("{key}: '{before}' -> '{after}' ({transition})")),
        BatchOutcome::Succeeded(TicketAdvance::WouldAdvance {
            before,
            after,
            transition,
        }) => Some(format!(
            "would advance {key}: '{before}' -> '{after}' ({transition})"
        )),
        BatchOutcome::Succeeded(TicketAdvance::MovedUnknownAfter {
            before, transition, ..
        }) => Some(format!("{key}: '{before}' -> '?' ({transition})")),
        BatchOutcome::Failed(_) | BatchOutcome::Skipped { .. } => None,
    }
}

pub(super) fn print_jira_advance(record: &AdvanceRecord) {
    if let Some(line) = format_jira_advance_line(record) {
        println!("{line}");
    }
    let key = &record.key;
    match &record.outcome {
        BatchOutcome::Succeeded(TicketAdvance::MovedUnknownAfter {
            transition,
            warning,
            ..
        }) => tracing::warn!(
            "{key}: transitioned via '{transition}', could not re-read status: {warning}"
        ),
        BatchOutcome::Failed(reason) => tracing::warn!("{key}: {reason}"),
        BatchOutcome::Skipped { reason } => tracing::warn!("{key}: skipped: {reason}"),
        BatchOutcome::Succeeded(
            TicketAdvance::Advanced { .. } | TicketAdvance::WouldAdvance { .. },
        ) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_advance_preview_line_matches_golden_format() {
        let line = format_jira_advance_line(&AdvanceRecord::new(
            "INTEL-1".into(),
            BatchOutcome::Succeeded(TicketAdvance::WouldAdvance {
                before: "Open".into(),
                after: "In Progress".into(),
                transition: "Start Progress".into(),
            }),
        ));
        assert_eq!(
            line.as_deref(),
            Some("would advance INTEL-1: 'Open' -> 'In Progress' (Start Progress)")
        );
    }
}
