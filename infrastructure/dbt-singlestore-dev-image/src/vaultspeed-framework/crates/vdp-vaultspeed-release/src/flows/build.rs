//! Release branch (`feature/<DROP>`) construction from `main`, zip branches, and PML scripts.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use vdp_git::{FileChange, PushMode};

use super::{FlowContext, MergeStep, check_lease, merge, origin, staged};
use crate::guard::{CommitCheck, EnvironmentCommits};
use crate::status::CommitRef;
use crate::{VsError, VsRules};

/// One PML script to add or replace (`content`) or remove (`None`), by file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PmlChange {
    pub name: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub content: Option<String>,
}

fn pml_changes(rules: &VsRules, changes: &[PmlChange]) -> Result<Vec<FileChange>, VsError> {
    changes
        .iter()
        .map(|change| {
            let name = change.name.trim();
            let valid = !name.is_empty()
                && !name.starts_with('.')
                && name.to_ascii_lowercase().ends_with(".sql")
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c));
            if !valid {
                return Err(VsError::Invalid(format!(
                    "`{name}` is not a PML script name (letters, digits, _ - . and ending in .sql)"
                )));
            }
            Ok(FileChange {
                path: format!("{}/{name}", rules.settings.pml_dir),
                content: change.content.clone().map(String::into_bytes),
            })
        })
        .collect()
}

/// Build or extend a release branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BuildRequest {
    /// `2026_DROP25.1`
    pub drop: String,
    /// Zip branches to merge in (`release/1182_FMC`), oldest first is best.
    pub zips: Vec<String>,
    #[serde(default)]
    pub pml: Vec<PmlChange>,
    /// Start again from `main` although the branch exists (a force push; what it drops is listed).
    #[serde(default)]
    pub recreate: bool,
    #[serde(default)]
    pub accept_losses: bool,
}

/// What building a release branch did (or, in a dry run, would do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BuildOutcome {
    pub branch: String,
    /// Where origin had the branch when this ran (`None`: it did not exist): the next run's lease.
    pub origin_head: Option<String>,
    pub base: String,
    pub steps: Vec<MergeStep>,
    pub pml_commit: Option<String>,
    /// Zips and PML scripts on the branch afterwards.
    pub zips: Vec<String>,
    pub pml: Vec<String>,
    /// Commits only the old branch had (recreate).
    pub losses: Vec<CommitRef>,
    pub head: String,
    pub pushed: bool,
}

fn validate_zips(
    ctx: &FlowContext<'_>,
    owned: &EnvironmentCommits,
    zips: &[String],
) -> Result<(), VsError> {
    let mut seen = BTreeSet::new();
    for zip in zips {
        if !ctx.rules.is_zip_branch(zip) {
            return Err(VsError::Invalid(format!(
                "{zip} is not a zip branch ({}<n>_<TYPE>)",
                ctx.rules.settings.zip_prefix
            )));
        }
        if !seen.insert(zip) {
            return Err(VsError::Invalid(format!("{zip} is listed twice")));
        }
        if ctx.vcs.remote_head(zip)?.is_none() {
            return Err(VsError::Invalid(format!("{zip} does not exist on origin")));
        }
        owned.check(
            ctx.vcs,
            CommitCheck {
                rules: ctx.rules,
                rev: &origin(zip),
                what: zip,
            },
        )?;
    }
    Ok(())
}

/// Builds the release branch of `request.drop`: starts from `main` (or from the branch on origin), merges the zip
/// branches, commits the PML scripts and pushes with a lease. A dry run pushes nothing.
///
/// `ctx.expect`: what the preview saw of the branch on origin (`None`: absent), ignored in a dry run.
///
/// # Errors
///
/// [`VsError::Stale`] when origin's branch moved since the preview (`ctx.expect`; not checked in a dry run);
/// [`VsError::Invalid`] when `request.drop` is not a release name; [`VsError::Refused`] when a zip or the branch holds
/// environment content or recreating drops commits that were not accepted; [`VsError::Aborted`] when the release manager
/// stops at a conflict; [`VsError::Zip`] when a zip cannot be read; [`VsError::Git`] when a git operation fails (including a rejected push).
pub fn build_release(
    ctx: &FlowContext<'_>,
    request: &BuildRequest,
) -> Result<BuildOutcome, VsError> {
    let FlowContext {
        vcs,
        rules,
        expect,
        control,
    } = *ctx;
    vcs.fetch_all()?;
    let branch = rules.release_branch(&request.drop)?;
    let origin_head = if control.dry_run {
        vcs.remote_head(&branch)?
    } else {
        check_lease(vcs, &branch, expect)?
    };
    let main = origin(&rules.settings.main);
    let owned = EnvironmentCommits::load(vcs, rules)?;
    validate_zips(ctx, &owned, &request.zips)?;
    let start = match (&origin_head, request.recreate) {
        (Some(_), false) => {
            owned.check(
                vcs,
                CommitCheck {
                    rules,
                    rev: &origin(&branch),
                    what: &branch,
                },
            )?;
            origin(&branch)
        }
        _ => main.clone(),
    };
    vcs.checkout_reset(&branch, &start)?;
    let base = vcs.rev_parse("HEAD")?;
    (control.progress)(&format!("{branch} from {start}"));

    let mut steps = Vec::new();
    for zip in &request.zips {
        steps.push(merge(vcs, control, zip, &origin(zip), &branch)?);
    }
    let pml = pml_changes(rules, &request.pml)?;
    let pml_commit = if pml.is_empty() {
        None
    } else {
        let names: Vec<&str> = request.pml.iter().map(|p| p.name.as_str()).collect();
        let message = format!("PML for {}: {}", request.drop, names.join(", "));
        let commit = vcs.commit_changes(&pml, &message)?;
        (control.progress)(&format!("PML: {}", names.join(", ")));
        commit
    };
    let head = vcs.rev_parse("HEAD")?;
    let losses: Vec<CommitRef> = match (&origin_head, request.recreate) {
        (Some(old), true) => vcs
            .log(&head, old, false)?
            .iter()
            .map(CommitRef::from)
            .collect(),
        _ => Vec::new(),
    };
    let (zips, pml_files) = staged(vcs, rules, "HEAD")?;
    let mut outcome = BuildOutcome {
        branch: branch.clone(),
        origin_head: origin_head.clone(),
        base,
        steps,
        pml_commit,
        zips,
        pml: pml_files,
        losses,
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
    if !outcome.losses.is_empty() && !request.accept_losses {
        return Err(VsError::Refused(format!(
            "recreating {branch} drops {} commit(s) only the old branch has; tick to accept",
            outcome.losses.len()
        )));
    }
    if origin_head.as_deref() == Some(head.as_str()) {
        (control.progress)("nothing new to push");
        return Ok(outcome);
    }
    vcs.push(&branch, PushMode::Expect(origin_head))?;
    (control.progress)(&format!("pushed {branch}"));
    outcome.pushed = true;
    Ok(outcome)
}
