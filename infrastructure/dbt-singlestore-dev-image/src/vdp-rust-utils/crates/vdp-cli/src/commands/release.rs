//! `vdp release ...` and `vdp jira ...`

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::sync::Arc;

use anyhow::{Result, bail};
use vdp_core::{Context, Outcome};
use vdp_dbt::{DbtEngine, DbtProject, Invocation};
use vdp_git::Vcs;
use vdp_jira::{IssueTracker, JiraSettings};
use vdp_release::{
    Analyser, Analysis, AnalysisKind, AnalysisRequest, BranchRules, Classification, PatchFlow,
    Progress, PromoteOptions, Promotion, PromotionKind, ReleaseFlow, ReleaseOptions,
    ReleaseSettings, Step, ViolationVia, check_scope, local_name, release_branches, remote_ref,
};

use crate::args::{JiraCommand, PromoteKind, ReleaseCommand};
use crate::bootstrap::DbtSelection;

pub(super) fn release(ctx: &Context, command: &ReleaseCommand) -> Result<Outcome> {
    let settings: ReleaseSettings = ctx.config().section(ReleaseSettings::SECTION)?;
    match command {
        ReleaseCommand::Create { .. } => create(ctx, &settings, command),
        ReleaseCommand::Analyse { .. } => analyse_command(ctx, &settings, command),
        ReleaseCommand::Patch { .. } => patch(ctx, &settings, command),
        ReleaseCommand::Promote { .. } => promote(ctx, &settings, command),
        ReleaseCommand::Branches => branches(ctx, &settings),
        #[cfg(feature = "release-hub")]
        ReleaseCommand::Serve { .. } => super::hub::serve(ctx, settings, command),
    }
}

fn create(ctx: &Context, settings: &ReleaseSettings, command: &ReleaseCommand) -> Result<Outcome> {
    let ReleaseCommand::Create {
        name,
        fixversion,
        from,
        to,
        keep_empty_fixversions,
        interactive,
        force,
        accept_losses,
        no_changelog,
        allow_old_release,
        dry_run,
    } = command
    else {
        unreachable!("dispatched by variant")
    };
    let request = AnalysisRequest {
        kind: AnalysisKind::Release,
        source: from.clone().unwrap_or_else(|| settings.from.clone()),
        compare: to.clone().unwrap_or_else(|| settings.to.clone()),
        fix_version: fixversion.clone(),
    };
    let analysis = analyse(ctx, settings, &request, *keep_empty_fixversions, true)?;
    // The CLI has no board to look at: a ticket without Jira data stops the release.
    let errors = analysis.jira_errors();
    if !errors.is_empty() {
        let list: Vec<String> = errors.iter().map(|(id, e)| format!("{id}: {e}")).collect();
        bail!("Jira: {}", list.join("; "));
    }
    print_analysis(&analysis);
    let vcs = ctx.service::<Arc<dyn Vcs>>()?;
    let validate = || validate(ctx, &settings.validate);
    let rules = BranchRules::from_settings(settings);
    if !allow_old_release {
        check_scope(
            vcs.as_ref(),
            &rules,
            name,
            vdp_release::ScopeAction::Release { recreate: true },
        )?;
    }
    let flow = ReleaseFlow {
        vcs: vcs.as_ref(),
        rules: &rules,
        validate: Some(&validate),
        confirm: &confirm,
        progress: &|message| eprintln!("{message}"),
        conflicts: vdp_release::Conflicts::default(),
    };
    let outcome = flow.run(
        &analysis,
        &analysis.default_selection(),
        &ReleaseOptions {
            branch: name.clone(),
            force: *force,
            dry_run: *dry_run,
            interactive: *interactive,
            resume: true,
            expect: None,
            accept_losses: *accept_losses,
            changelog: !no_changelog,
        },
    )?;
    print_outcome(&outcome);
    Ok(Outcome::Success)
}

fn analyse_command(
    ctx: &Context,
    settings: &ReleaseSettings,
    command: &ReleaseCommand,
) -> Result<Outcome> {
    let ReleaseCommand::Analyse {
        fixversion,
        from,
        to,
        patch,
        json,
    } = command
    else {
        unreachable!("dispatched by variant")
    };
    let request = match patch {
        Some(branch) => AnalysisRequest {
            kind: AnalysisKind::Patch,
            source: from.clone().unwrap_or_else(|| settings.from.clone()),
            compare: remote_ref(&local_name(branch)),
            fix_version: fix_version_of(fixversion.as_deref(), branch, settings)?,
        },
        None => AnalysisRequest {
            kind: AnalysisKind::Release,
            source: from.clone().unwrap_or_else(|| settings.from.clone()),
            compare: to.clone().unwrap_or_else(|| settings.to.clone()),
            fix_version: fixversion
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--fixversion is required for a release"))?,
        },
    };
    let analysis = analyse(ctx, settings, &request, false, !json)?;
    if *json {
        println!("{}", serde_json::to_string_pretty(&analysis)?);
    } else {
        print_analysis(&analysis);
    }
    Ok(Outcome::Success)
}

fn patch(ctx: &Context, settings: &ReleaseSettings, command: &ReleaseCommand) -> Result<Outcome> {
    let ReleaseCommand::Patch {
        branch,
        include,
        fixversion,
        from,
        interactive,
        allow_old_release,
        no_changelog,
        dry_run,
    } = command
    else {
        unreachable!("dispatched by variant")
    };
    let request = AnalysisRequest {
        kind: AnalysisKind::Patch,
        source: from.clone().unwrap_or_else(|| settings.from.clone()),
        compare: remote_ref(&local_name(branch)),
        fix_version: fix_version_of(fixversion.as_deref(), branch, settings)?,
    };
    let analysis = analyse(ctx, settings, &request, false, true)?;
    print_analysis(&analysis);
    let selection: BTreeSet<String> = match include {
        Some(ids) => ids.iter().map(|id| id.trim().to_uppercase()).collect(),
        None => analysis.default_selection(),
    };
    let vcs = ctx.service::<Arc<dyn Vcs>>()?;
    let rules = BranchRules::from_settings(settings);
    if !allow_old_release {
        check_scope(
            vcs.as_ref(),
            &rules,
            branch,
            vdp_release::ScopeAction::Patch,
        )?;
    }
    let flow = PatchFlow {
        vcs: vcs.as_ref(),
        rules: &rules,
        confirm: &confirm,
        progress: &|message| eprintln!("{message}"),
        conflicts: vdp_release::Conflicts::default(),
        changelog: !no_changelog,
    };
    print_outcome(&flow.run(&analysis, &selection, *dry_run, *interactive)?);
    Ok(Outcome::Success)
}

fn promote(ctx: &Context, settings: &ReleaseSettings, command: &ReleaseCommand) -> Result<Outcome> {
    let ReleaseCommand::Promote {
        branch,
        kind,
        accept_rollback,
        accept_losses,
        allow_old_release,
        dry_run,
    } = command
    else {
        unreachable!("dispatched by variant")
    };
    let vcs = ctx.service::<Arc<dyn Vcs>>()?;
    let rules = BranchRules::from_settings(settings);
    let promotion = Promotion {
        vcs: vcs.as_ref(),
        rules: &rules,
        progress: &|message| eprintln!("{message}"),
        conflicts: vdp_release::Conflicts::default(),
    };
    let kind = match kind {
        PromoteKind::Auto => None,
        PromoteKind::New => Some(PromotionKind::New),
        PromoteKind::Patch => Some(PromotionKind::Patch),
    };
    if !allow_old_release {
        check_scope(
            vcs.as_ref(),
            &rules,
            branch,
            vdp_release::ScopeAction::Promote,
        )?;
    }
    let done = promotion.run(
        branch,
        &PromoteOptions {
            kind,
            dry_run: *dry_run,
            expect_main: None,
            accept_rollback: *accept_rollback,
            accept_losses: *accept_losses,
        },
    )?;
    if done.nothing_to_promote {
        println!(
            "nothing to promote: {} is already on {}",
            done.branch, rules.main
        );
        return Ok(Outcome::Success);
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
        local_name(&settings.to),
        done.kind.unwrap_or(PromotionKind::New),
        done.reason,
        done.main_head
            .map(|h| format!(", main is now {}", &h[..h.len().min(12)]))
            .unwrap_or_default()
    );
    Ok(Outcome::Success)
}

fn branches(ctx: &Context, settings: &ReleaseSettings) -> Result<Outcome> {
    let vcs = ctx.service::<Arc<dyn Vcs>>()?;
    if let Err(error) = vcs.fetch_all() {
        eprintln!("fetch failed (showing local refs): {error}");
    }
    for branch in release_branches(vcs.as_ref(), &settings.branch_prefix, &settings.to)? {
        println!(
            "{:<28} {} {}",
            branch.name,
            &branch.head[..branch.head.len().min(10)],
            if branch.promoted { "on main" } else { "" }
        );
    }
    Ok(Outcome::Success)
}

/// The fix version of a patch: given, else the release branch's suffix.
fn fix_version_of(given: Option<&str>, branch: &str, settings: &ReleaseSettings) -> Result<String> {
    if let Some(version) = given {
        return Ok(version.to_owned());
    }
    let local = local_name(branch);
    let suffix = local
        .strip_prefix(&settings.branch_prefix)
        .unwrap_or(&local);
    suffix
        .parse::<vdp_jira::FixVersion>()
        .map(|v| v.name().to_owned())
        .map_err(|_| anyhow::anyhow!("{branch} has no fix version suffix; pass --fixversion"))
}

/// Runs an analysis with progress on stderr; Jira is optional (tickets then show a Jira error).
fn analyse(
    ctx: &Context,
    settings: &ReleaseSettings,
    request: &AnalysisRequest,
    keep_empty: bool,
    verbose: bool,
) -> Result<Analysis> {
    let jira_settings: JiraSettings = ctx.config().section(JiraSettings::SECTION)?;
    let vcs = ctx.service::<Arc<dyn Vcs>>()?;
    let tracker = ctx.service::<Arc<dyn IssueTracker>>().ok();
    if let Err(error) = vcs.fetch_all() {
        eprintln!("fetch failed (continuing with local refs): {error}");
    }
    let progress = |p: Progress| {
        if verbose {
            match p {
                Progress::Step(message) => eprintln!("{message}"),
                Progress::Jira(done, total, key) => eprintln!("jira {done}/{total} {key}"),
            }
        }
    };
    let rules = BranchRules::from_settings(settings);
    let analyser = Analyser {
        vcs: vcs.as_ref(),
        rules: &rules,
        tracker: tracker.as_deref(),
        jira_base_url: jira_settings.base_url.clone(),
        ticket_prefix: settings.ticket_prefix.clone(),
        excluded: jira_settings.excluded_fix_versions,
        keep_empty,
        progress: &progress,
    };
    Ok(analyser.analyse(request)?)
}

/// Reverts on the source (dev), which must never have one: reported, never repeated.
fn print_violations(analysis: &Analysis) {
    if analysis.dev_violations.is_empty() {
        return;
    }
    eprintln!(
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
        eprintln!(
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

fn print_analysis(analysis: &Analysis) {
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

fn print_outcome(outcome: &vdp_release::Outcome) {
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

/// Runs the configured dbt validation in the project.
fn validate(ctx: &Context, args: &[String]) -> Result<(), String> {
    let project = ctx
        .service::<Arc<DbtProject>>()
        .map_err(|e| e.to_string())?;
    let engine = ctx
        .service::<Arc<dyn DbtEngine>>()
        .map_err(|e| e.to_string())?;
    let selection = ctx.service::<DbtSelection>().map_err(|e| e.to_string())?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    eprintln!("validating: dbt {}", args.join(" "));
    let done = engine
        .invoke(&Invocation {
            args: selection.args(&project.root, &args),
            cwd: Some(project.root.clone()),
            capture: true,
        })
        .map_err(|e| e.to_string())?;
    if done.success() {
        return Ok(());
    }
    let output = done.stdout_text();
    let errors: Vec<&str> = output
        .lines()
        .filter(|l| l.contains("[error]"))
        .take(10)
        .collect();
    Err(if errors.is_empty() {
        format!("dbt exited with {}", done.exit_code())
    } else {
        errors.join("\n")
    })
}

fn confirm(step: &Step, index: usize, total: usize) -> bool {
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

pub(super) fn jira(ctx: &Context, command: &JiraCommand) -> Result<Outcome> {
    let JiraCommand::Advance {
        tickets,
        transition,
    } = command;
    let settings: JiraSettings = ctx.config().section(JiraSettings::SECTION)?;
    let wanted = transition
        .clone()
        .unwrap_or_else(|| settings.advance_transition.clone());
    let tracker = ctx.service::<Arc<dyn IssueTracker>>()?;
    let mut ok = true;
    for key in tickets {
        let before = tracker.ticket(key)?;
        let options = tracker.transitions(key)?;
        let chosen = if wanted.is_empty() {
            options.first()
        } else {
            options.iter().find(|t| {
                t.name.eq_ignore_ascii_case(&wanted) || t.to.eq_ignore_ascii_case(&wanted)
            })
        };
        let Some(chosen) = chosen else {
            ok = false;
            let names: Vec<&str> = options.iter().map(|t| t.name.as_str()).collect();
            eprintln!(
                "{key}: no transition {wanted:?} from '{}' (offered: {})",
                before.status,
                names.join(", ")
            );
            continue;
        };
        tracker.transition(key, &chosen.id)?;
        let after = tracker.ticket(key)?;
        println!(
            "{key}: '{}' -> '{}' ({})",
            before.status, after.status, chosen.name
        );
    }
    if !ok {
        bail!("some tickets were not moved");
    }
    Ok(Outcome::Success)
}
