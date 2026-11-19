//! `vdp release ...` and `vdp jira ...`

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::{Result, bail};
use vdp_core::{Context, Outcome};
use vdp_git::Vcs;
use vdp_jira::{IssueTracker, JiraSettings};
use vdp_release::{
    Analyser, Analysis, AnalysisKind, AnalysisRequest, BranchRules, PatchFlow, Progress,
    PromoteOptions, Promotion, PromotionKind, ReleaseFlow, ReleaseOptions, ReleaseSettings,
    check_scope, local_name, release_branches, remote_ref,
};

use super::common::validate_project;
use super::release_present::{
    confirm, print_analysis, print_branches, print_jira_advance, print_outcome, print_promotion,
};
use crate::args::{JiraCommand, PromoteKind, ReleaseCommand};

pub(super) fn release(ctx: &Context, command: &ReleaseCommand) -> Result<Outcome> {
    let settings: ReleaseSettings = ctx.config().section(ReleaseSettings::SECTION)?;
    match command {
        ReleaseCommand::Create { .. } => create(ctx, &settings, command),
        ReleaseCommand::Analyse { .. } => analyse_command(ctx, &settings, command),
        ReleaseCommand::Patch { .. } => patch(ctx, &settings, command),
        ReleaseCommand::Promote { .. } => promote(ctx, &settings, command),
        ReleaseCommand::Branches => branches(ctx, &settings),
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
    let validate = || validate_project(ctx, &settings.validate);
    let rules = BranchRules::from_settings(settings);
    if !allow_old_release {
        check_scope(
            vcs.as_ref(),
            &vdp_release::ScopeCheck {
                rules: &rules,
                branch: name,
                action: vdp_release::ScopeAction::Release { recreate: true },
            },
        )?;
    }
    let flow = ReleaseFlow {
        vcs: vcs.as_ref(),
        rules: &rules,
        validate: Some(&validate),
        confirm: &confirm,
        progress: &|message| tracing::info!("{message}"),
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
    let rules = BranchRules::from_settings(settings);
    let request = match patch {
        Some(branch) => AnalysisRequest {
            kind: AnalysisKind::Patch,
            source: from.clone().unwrap_or_else(|| settings.from.clone()),
            compare: remote_ref(&local_name(branch)),
            fix_version: rules.fix_version_of(fixversion.as_deref(), branch)?,
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
    let rules = BranchRules::from_settings(settings);
    let request = AnalysisRequest {
        kind: AnalysisKind::Patch,
        source: from.clone().unwrap_or_else(|| settings.from.clone()),
        compare: remote_ref(&local_name(branch)),
        fix_version: rules.fix_version_of(fixversion.as_deref(), branch)?,
    };
    let analysis = analyse(ctx, settings, &request, false, true)?;
    print_analysis(&analysis);
    let selection: BTreeSet<String> = match include {
        Some(ids) => ids.iter().map(|id| id.trim().to_uppercase()).collect(),
        None => analysis.default_selection(),
    };
    let vcs = ctx.service::<Arc<dyn Vcs>>()?;
    if !allow_old_release {
        check_scope(
            vcs.as_ref(),
            &vdp_release::ScopeCheck {
                rules: &rules,
                branch,
                action: vdp_release::ScopeAction::Patch,
            },
        )?;
    }
    let flow = PatchFlow {
        vcs: vcs.as_ref(),
        rules: &rules,
        confirm: &confirm,
        progress: &|message| tracing::info!("{message}"),
        conflicts: vdp_release::Conflicts::default(),
        changelog: !no_changelog,
    };
    print_outcome(&flow.run(
        &analysis,
        &selection,
        &vdp_release::PatchOptions {
            dry_run: *dry_run,
            interactive: *interactive,
        },
    )?);
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
        progress: &|message| tracing::info!("{message}"),
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
            &vdp_release::ScopeCheck {
                rules: &rules,
                branch,
                action: vdp_release::ScopeAction::Promote,
            },
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
    print_promotion(&done, &rules, &settings.to);
    Ok(Outcome::Success)
}

fn branches(ctx: &Context, settings: &ReleaseSettings) -> Result<Outcome> {
    let vcs = ctx.service::<Arc<dyn Vcs>>()?;
    if let Err(error) = vcs.fetch_all() {
        tracing::warn!(
            "fetch failed (showing local refs): {}",
            vdp_core::display_chain(&error)
        );
    }
    let list = release_branches(vcs.as_ref(), &settings.branch_prefix, &settings.to)?;
    print_branches(&list);
    Ok(Outcome::Success)
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
        tracing::warn!(
            "fetch failed (continuing with local refs): {}",
            vdp_core::display_chain(&error)
        );
    }
    let progress = |p: Progress| {
        if verbose {
            match p {
                Progress::Step(message) => tracing::info!("{message}"),
                Progress::Jira(done, total, key) => tracing::info!("jira {done}/{total} {key}"),
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

pub(super) fn jira(ctx: &Context, command: &JiraCommand) -> Result<Outcome> {
    let JiraCommand::Advance {
        tickets,
        transition,
        dry_run,
    } = command;
    let settings: JiraSettings = ctx.config().section(JiraSettings::SECTION)?;
    let wanted = transition
        .as_deref()
        .unwrap_or(&settings.advance_transition);
    let tracker = ctx.service::<Arc<dyn IssueTracker>>()?;
    let report = vdp_jira::advance_tickets_with(
        tracker.as_ref(),
        &vdp_jira::AdvanceRequest {
            tickets,
            wanted_transition: wanted,
            dry_run: *dry_run,
        },
        print_jira_advance,
    )?;
    Ok(report.outcome())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use vdp_core::Config;
    use vdp_jira::{JiraError, Ticket, Transition};

    use super::*;

    #[derive(Debug, Default)]
    struct FakeTracker {
        failing_key: String,
        fail_after_read_key: String,
        fatal_error: Option<(String, JiraError)>,
        requested: Mutex<Vec<String>>,
        moved: Mutex<Vec<(String, String)>>,
        done: Mutex<HashSet<String>>,
    }

    impl FakeTracker {
        fn failing(key: &str) -> Arc<Self> {
            Arc::new(Self {
                failing_key: key.to_owned(),
                ..Self::default()
            })
        }

        fn failing_followup(key: &str) -> Arc<Self> {
            Arc::new(Self {
                fail_after_read_key: key.to_owned(),
                ..Self::default()
            })
        }

        fn fatal(key: &str, err: JiraError) -> Arc<Self> {
            Arc::new(Self {
                fatal_error: Some((key.to_owned(), err)),
                ..Self::default()
            })
        }

        fn requested(&self) -> Vec<String> {
            self.requested.lock().unwrap().clone()
        }

        fn transitioned(&self) -> Vec<(String, String)> {
            self.moved.lock().unwrap().clone()
        }
    }

    impl IssueTracker for FakeTracker {
        fn ticket(&self, key: &str) -> Result<Ticket, JiraError> {
            self.requested.lock().unwrap().push(key.to_owned());
            if let Some((target, err)) = &self.fatal_error
                && target == key
            {
                return Err(err.clone());
            }
            if key == self.failing_key {
                return Err(JiraError::Request {
                    url: format!("https://jira.example/rest/api/2/issue/{key}"),
                    detail: "HTTP 500 Internal Server Error".into(),
                });
            }
            let is_done = self.done.lock().unwrap().contains(key);
            if is_done && key == self.fail_after_read_key {
                return Err(JiraError::Request {
                    url: format!("https://jira.example/rest/api/2/issue/{key}"),
                    detail: "HTTP 500 on follow-up read".into(),
                });
            }
            let status = if is_done { "In Progress" } else { "Open" };
            Ok(Ticket {
                key: key.to_owned(),
                status: status.to_owned(),
                ..Ticket::default()
            })
        }

        fn transitions(&self, _key: &str) -> Result<Vec<Transition>, JiraError> {
            Ok(vec![Transition {
                id: "11".into(),
                name: "Start Progress".into(),
                to: "In Progress".into(),
            }])
        }

        fn transition(&self, key: &str, transition_id: &str) -> Result<(), JiraError> {
            self.moved
                .lock()
                .unwrap()
                .push((key.to_owned(), transition_id.to_owned()));
            self.done.lock().unwrap().insert(key.to_owned());
            Ok(())
        }
    }

    #[test]
    fn jira_advance_isolates_per_ticket_failures_and_returns_failures_outcome() {
        let tracker = FakeTracker::failing("INTEL-2");
        let ctx = Context::builder(Config::default())
            .provide_value(tracker.clone() as Arc<dyn IssueTracker>)
            .build();
        let outcome = super::jira(
            &ctx,
            &JiraCommand::Advance {
                tickets: vec!["INTEL-1".into(), "INTEL-2".into(), "INTEL-3".into()],
                transition: None,
                dry_run: false,
            },
        )
        .expect("per-ticket Jira failures should return Outcome::Failures, not Err");
        assert_eq!(outcome, Outcome::Failures);
        assert_eq!(outcome.exit_code(), 1);
        assert_eq!(
            tracker.transitioned(),
            [
                ("INTEL-1".into(), "11".into()),
                ("INTEL-3".into(), "11".into()),
            ]
        );
    }

    #[test]
    fn jira_advance_returns_success_when_all_tickets_transition() {
        let tracker = FakeTracker::failing("NONE");
        let ctx = Context::builder(Config::default())
            .provide_value(tracker.clone() as Arc<dyn IssueTracker>)
            .build();
        let outcome = super::jira(
            &ctx,
            &JiraCommand::Advance {
                tickets: vec!["INTEL-1".into(), "INTEL-3".into()],
                transition: Some("In Progress".into()),
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(outcome.exit_code(), 0);
        assert_eq!(
            tracker.transitioned(),
            [
                ("INTEL-1".into(), "11".into()),
                ("INTEL-3".into(), "11".into()),
            ]
        );
    }

    #[test]
    fn jira_advance_returns_err_on_auth_or_transport_failure() {
        let tracker = FakeTracker::fatal(
            "INTEL-1",
            JiraError::Auth {
                url: "https://jira.example/rest/api/2/issue/INTEL-1".into(),
                status: 403,
                detail: "HTTP 403".into(),
            },
        );
        let ctx = Context::builder(Config::default())
            .provide_value(tracker.clone() as Arc<dyn IssueTracker>)
            .build();
        let err = super::jira(
            &ctx,
            &JiraCommand::Advance {
                tickets: vec!["INTEL-1".into(), "INTEL-2".into()],
                transition: None,
                dry_run: false,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("403"), "{err}");
        assert_eq!(tracker.requested(), ["INTEL-1"]);
    }

    #[test]
    fn jira_advance_exits_zero_when_followup_read_fails_after_transition() {
        use std::fmt::Debug;

        use tracing::Subscriber;
        use tracing::field::{Field, Visit};
        use tracing_subscriber::layer::{Context as LayerContext, Layer, SubscriberExt as _};

        #[derive(Default)]
        struct MsgVisitor(Option<String>);

        impl Visit for MsgVisitor {
            fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
                if field.name() == "message" {
                    self.0 = Some(format!("{value:?}"));
                }
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "message" {
                    self.0 = Some(value.to_owned());
                }
            }
        }

        struct WarnLayer(Arc<Mutex<Vec<String>>>);

        impl<S: Subscriber> Layer<S> for WarnLayer {
            fn on_event(&self, event: &tracing::Event<'_>, _ctx: LayerContext<'_, S>) {
                if *event.metadata().level() != tracing::Level::WARN {
                    return;
                }
                let mut visitor = MsgVisitor::default();
                event.record(&mut visitor);
                if let Some(msg) = visitor.0 {
                    self.0.lock().unwrap().push(msg);
                }
            }
        }

        let tracker = FakeTracker::failing_followup("INTEL-1");
        let ctx = Context::builder(Config::default())
            .provide_value(tracker.clone() as Arc<dyn IssueTracker>)
            .build();
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(WarnLayer(Arc::clone(&warnings)));

        let outcome = tracing::subscriber::with_default(subscriber, || {
            super::jira(
                &ctx,
                &JiraCommand::Advance {
                    tickets: vec!["INTEL-1".into()],
                    transition: None,
                    dry_run: false,
                },
            )
            .unwrap()
        });
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(outcome.exit_code(), 0);
        assert_eq!(tracker.transitioned(), [("INTEL-1".into(), "11".into())]);

        let warnings = warnings.lock().unwrap();
        assert_eq!(warnings.len(), 1, "expected 1 warning, got: {warnings:?}");
        assert!(
            warnings[0].starts_with(
                "INTEL-1: transitioned via 'Start Progress', could not re-read status:"
            ),
            "unexpected warning format: {:?}",
            warnings[0]
        );
        assert!(
            !warnings[0].contains("warning:"),
            "duplicate 'warning:' prefix in {:?}",
            warnings[0]
        );
    }

    #[test]
    fn jira_advance_dry_run_resolves_without_mutating_tracker() {
        let tracker = FakeTracker::failing("NONE");
        let ctx = Context::builder(Config::default())
            .provide_value(tracker.clone() as Arc<dyn IssueTracker>)
            .build();
        let outcome = super::jira(
            &ctx,
            &JiraCommand::Advance {
                tickets: vec!["INTEL-1".into(), "INTEL-3".into()],
                transition: Some("In Progress".into()),
                dry_run: true,
            },
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(tracker.transitioned(), []);
        assert_eq!(tracker.requested(), ["INTEL-1", "INTEL-3"]);
    }
}
