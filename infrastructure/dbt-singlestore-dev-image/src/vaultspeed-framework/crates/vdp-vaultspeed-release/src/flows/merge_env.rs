//! Environment merge flows (`LDV`, `ABT`, `CBT`, `PRD`) with lower-environment deployment warnings.

use serde::{Deserialize, Serialize};
use vdp_git::{PushMode, Vcs};

use super::{FlowContext, MergeStep, check_lease, merge, origin, staged};
use crate::guard::{CommitCheck, EnvironmentCommits};
use crate::{VsError, VsRules};

/// Bring release branches (or, where allowed, single zip branches) into an environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MergeRequest {
    pub environment: String,
    /// In order; the last merge's message is what CI sees.
    pub sources: Vec<String>,
}

/// A zip arriving in an environment that has not been deployed on the environment below it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LowerWarning {
    pub zip: String,
    pub lower: String,
    pub detail: String,
}

/// What merging into an environment did (or, in a dry run, would do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MergeOutcome {
    pub environment: String,
    /// Where origin had the environment when this ran: the next run's lease.
    pub origin_head: String,
    pub steps: Vec<MergeStep>,
    /// Zips and PML scripts the merges stage (what CI will deploy).
    pub incoming_zips: Vec<String>,
    pub incoming_pml: Vec<String>,
    /// Not blocking: zips not yet deployed on the environment below.
    pub warnings: Vec<LowerWarning>,
    /// Whether the last commit's message makes CI deploy.
    pub triggers_deploy: bool,
    pub head: String,
    pub pushed: bool,
}

/// Only release branches (and, for environments that take them, zip branches) that exist and carry no
/// environment's commits are merged into an environment.
fn check_sources(
    vcs: &dyn Vcs,
    rules: &VsRules,
    env: &str,
    sources: &[String],
) -> Result<(), VsError> {
    let direct = rules
        .settings
        .direct_merge_environments
        .iter()
        .any(|e| e == env);
    let owned = EnvironmentCommits::load(vcs, rules)?;
    for source in sources {
        let allowed = rules.is_release_branch(source) || (direct && rules.is_zip_branch(source));
        if !allowed {
            return Err(VsError::Refused(format!(
                "{source} cannot be merged into {env}: {}",
                if rules.is_zip_branch(source) {
                    format!(
                        "{env} only receives release branches ({}<DROP>)",
                        rules.settings.release_prefix
                    )
                } else {
                    "only release branches and zip branches are merged into environments".to_owned()
                }
            )));
        }
        if vcs.remote_head(source)?.is_none() {
            return Err(VsError::Invalid(format!(
                "{source} does not exist on origin"
            )));
        }
        owned.check(
            vcs,
            CommitCheck {
                rules,
                rev: &origin(source),
                what: source,
            },
        )?;
    }
    Ok(())
}

/// Merges each source into an environment branch and pushes with a lease; the merge commits are what CI deploys on.
/// A dry run pushes nothing.
///
/// `ctx.expect`: the environment's head the preview saw. `deployed_below`: per zip, whether the environment below
/// deployed it (for the warnings; the hub passes what its status knows).
///
/// # Errors
///
/// [`VsError::Stale`] when origin's branch moved since the preview (`ctx.expect`; not checked in a dry run);
/// [`VsError::Invalid`] when the target is not an environment, there is nothing to merge or the environment does not exist
/// on origin; [`VsError::Refused`] when a source is not allowed into the environment; [`VsError::Aborted`] when the
/// release manager stops at a conflict; [`VsError::Zip`] when a zip cannot be read; [`VsError::Git`] when a git operation fails (including a rejected push).
pub fn merge_into_environment(
    ctx: &FlowContext<'_>,
    request: &MergeRequest,
    deployed_below: &dyn Fn(&str, &str) -> bool,
) -> Result<MergeOutcome, VsError> {
    let FlowContext {
        vcs,
        rules,
        expect,
        control,
    } = *ctx;
    vcs.fetch_all()?;
    let env = request.environment.as_str();
    if !rules.is_environment(env) {
        return Err(VsError::Invalid(format!(
            "{env} is not an environment ({})",
            rules.environments().join(", ")
        )));
    }
    if request.sources.is_empty() {
        return Err(VsError::Invalid("nothing to merge".into()));
    }
    let origin_head = if control.dry_run {
        vcs.remote_head(env)?
    } else {
        check_lease(vcs, env, expect)?
    }
    .ok_or_else(|| VsError::Invalid(format!("{env} does not exist on origin")))?;
    check_sources(vcs, rules, env, &request.sources)?;

    vcs.checkout_reset(env, &origin(env))?;
    let (before_zips, before_pml) = staged(vcs, rules, "HEAD")?;
    let mut steps = Vec::new();
    for source in &request.sources {
        steps.push(merge(vcs, control, source, &origin(source), env)?);
    }
    let head = vcs.rev_parse("HEAD")?;
    let (after_zips, after_pml) = staged(vcs, rules, "HEAD")?;
    let incoming_zips: Vec<String> = after_zips
        .into_iter()
        .filter(|z| !before_zips.contains(z))
        .collect();
    let incoming_pml: Vec<String> = after_pml
        .into_iter()
        .filter(|p| !before_pml.contains(p))
        .collect();
    let warnings = rules
        .lower(env)
        .map(|lower| {
            incoming_zips
                .iter()
                .filter(|zip| !deployed_below(zip, lower))
                .map(|zip| LowerWarning {
                    zip: zip.clone(),
                    lower: lower.to_owned(),
                    detail: format!("{zip} has not been deployed on {lower}"),
                })
                .collect()
        })
        .unwrap_or_default();
    let last = vcs.commit("HEAD")?;
    let mut outcome = MergeOutcome {
        environment: env.to_owned(),
        origin_head: origin_head.clone(),
        steps,
        incoming_zips,
        incoming_pml,
        warnings,
        triggers_deploy: head != origin_head && VsRules::triggers_deploy(&last.message),
        head: head.clone(),
        pushed: false,
    };
    if control.dry_run {
        for step in &mut outcome.steps {
            if step.outcome == "merged" {
                step.outcome = "planned".into();
            }
        }
        return Ok(outcome);
    }
    if head == origin_head {
        (control.progress)("nothing new to merge");
        return Ok(outcome);
    }
    vcs.push(env, PushMode::Expect(Some(origin_head)))?;
    (control.progress)(&format!("pushed {env}: CI deploys"));
    outcome.pushed = true;
    Ok(outcome)
}
