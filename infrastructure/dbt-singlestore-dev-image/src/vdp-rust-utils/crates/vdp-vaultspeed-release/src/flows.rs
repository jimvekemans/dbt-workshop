//! The four things the release manager does with the framework repository, each previewed first (`dry_run`) and
//! then executed with a lease on exactly what the preview saw.
//!
//! - [`build_release`]: a `feature/<DROP>` branch from `main` with the chosen zip branches merged in and PML scripts.
//! - [`merge_into_environment`]: a release branch (or, where allowed, single zip branches) into an environment,
//!   with the merge message CI deploys on.
//! - [`commit_files`]: a direct fix on an environment, `main` (skeleton only) or a release branch.
//! - [`reset_branch`]: a branch back to an earlier commit (a force push, with the commits it drops listed).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use vdp_git::{Applied, FileChange, Operation, PushMode, Vcs};
use vdp_release::{Conflicts, ResolutionAction, plain_report};

use crate::guard::EnvironmentCommits;
use crate::status::CommitRef;
use crate::zips::parse_zip_name;
use crate::{VsError, VsRules};

fn origin(branch: &str) -> String {
    format!("origin/{branch}")
}

/// Refuses when origin moved since the preview (`expect` = what the preview saw; `None` = absent).
fn check_lease(
    vcs: &dyn Vcs,
    branch: &str,
    expect: Option<&String>,
) -> Result<Option<String>, VsError> {
    let now = vcs.remote_head(branch)?;
    if now.as_ref() != expect {
        let seen = |h: Option<&String>| {
            h.map_or_else(|| "absent".to_owned(), |h| h.chars().take(8).collect())
        };
        return Err(VsError::Stale(format!(
            "{branch} moved since the preview (preview: {}, origin now: {}): preview again",
            seen(expect),
            seen(now.as_ref())
        )));
    }
    Ok(now)
}

/// What happened to one source branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MergeStep {
    pub source: String,
    /// `merged`, `resolved` (after a conflict decision), `already` (nothing new), `skipped` (left out at a
    /// conflict) or `planned` (preview).
    pub outcome: String,
    pub message: String,
    pub files: Vec<String>,
}

/// How a run behaves: the preview flag, conflict handling and progress messages.
#[derive(Clone, Copy)]
pub struct RunControl<'a> {
    pub dry_run: bool,
    pub conflicts: Conflicts<'a>,
    pub progress: &'a dyn Fn(&str),
}

impl std::fmt::Debug for RunControl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunControl")
            .field("dry_run", &self.dry_run)
            .field("conflicts", &self.conflicts)
            .finish_non_exhaustive()
    }
}

/// Merges `source` (a remote-tracking ref) into the checked-out branch with `message`, pausing on conflicts.
fn merge(
    vcs: &dyn Vcs,
    control: &RunControl<'_>,
    source: &str,
    rev: &str,
    target: &str,
) -> Result<MergeStep, VsError> {
    let message = VsRules::merge_message(source, target);
    let before = vcs.rev_parse("HEAD")?;
    if vcs.is_ancestor(rev, "HEAD")? {
        return Ok(MergeStep {
            source: source.to_owned(),
            outcome: "already".into(),
            message,
            files: Vec::new(),
        });
    }
    let mut applied = vcs.merge_with(rev, &message, control.conflicts.options)?;
    let mut outcome = "merged";
    loop {
        match applied {
            Applied::Conflicted(conflict) => {
                let Some(resolve) = control.conflicts.resolve else {
                    vcs.abort_op(Operation::Merge)?;
                    return Err(VsError::Aborted(format!(
                        "merging {source} into {target} conflicts in {}",
                        conflict
                            .paths
                            .iter()
                            .map(|p| p.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                };
                let report = plain_report(&format!("merge {source} into {target}"), &conflict);
                (control.progress)(&format!(
                    "conflict merging {source}: {} file(s); waiting for a decision",
                    report.files.len()
                ));
                let resolution = resolve(&report);
                match resolution.action {
                    ResolutionAction::Continue => {
                        for (path, choice) in &resolution.choices {
                            vcs.resolve_path(path, (*choice).into())?;
                        }
                        applied = vcs.continue_op(Operation::Merge, rev)?;
                        outcome = "resolved";
                    }
                    ResolutionAction::SkipStep => {
                        vcs.abort_op(Operation::Merge)?;
                        (control.progress)(&format!("{source} left out"));
                        return Ok(MergeStep {
                            source: source.to_owned(),
                            outcome: "skipped".into(),
                            message,
                            files: Vec::new(),
                        });
                    }
                    ResolutionAction::Abort => {
                        vcs.abort_op(Operation::Merge)?;
                        return Err(VsError::Aborted(format!(
                            "merge of {source} stopped; nothing was pushed"
                        )));
                    }
                }
            }
            Applied::Empty => {
                outcome = "already";
                break;
            }
            Applied::Committed | Applied::Resolved(_) => break,
        }
    }
    let files = changed_files(vcs, &before, "HEAD")?;
    (control.progress)(&format!("merged {source} ({} file(s))", files.len()));
    Ok(MergeStep {
        source: source.to_owned(),
        outcome: outcome.into(),
        message,
        files,
    })
}

fn changed_files(vcs: &dyn Vcs, from: &str, to: &str) -> Result<Vec<String>, VsError> {
    let mut files = BTreeSet::new();
    for entry in vcs.path_log(from, to, ".")? {
        files.extend(entry.files.into_iter().map(|f| f.path));
    }
    Ok(files.into_iter().collect())
}

/// The zips and PML scripts staged at `rev`.
fn staged(
    vcs: &dyn Vcs,
    rules: &VsRules,
    rev: &str,
) -> Result<(Vec<String>, Vec<String>), VsError> {
    let s = &rules.settings;
    let zips = vcs
        .files_at(rev, &s.staging_dir)?
        .iter()
        .filter_map(|p| parse_zip_name(p).map(|(n, _, _)| n))
        .collect();
    let pml = vcs
        .files_at(rev, &s.pml_dir)?
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    Ok((zips, pml))
}

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

/// `expect`: what the preview saw of the branch on origin (`None`: absent), ignored in a dry run.
pub fn build_release(
    vcs: &dyn Vcs,
    rules: &VsRules,
    request: &BuildRequest,
    expect: Option<&String>,
    control: &RunControl<'_>,
) -> Result<BuildOutcome, VsError> {
    vcs.fetch_all()?;
    let branch = rules.release_branch(&request.drop)?;
    let origin_head = if control.dry_run {
        vcs.remote_head(&branch)?
    } else {
        check_lease(vcs, &branch, expect)?
    };
    let main = origin(&rules.settings.main);
    let owned = EnvironmentCommits::load(vcs, rules)?;
    let mut seen = BTreeSet::new();
    for zip in &request.zips {
        if !rules.is_zip_branch(zip) {
            return Err(VsError::Invalid(format!(
                "{zip} is not a zip branch ({}<n>_<TYPE>)",
                rules.settings.zip_prefix
            )));
        }
        if !seen.insert(zip) {
            return Err(VsError::Invalid(format!("{zip} is listed twice")));
        }
        if vcs.remote_head(zip)?.is_none() {
            return Err(VsError::Invalid(format!("{zip} does not exist on origin")));
        }
        owned.check(vcs, rules, &origin(zip), zip)?;
    }
    let start = match (&origin_head, request.recreate) {
        (Some(_), false) => {
            owned.check(vcs, rules, &origin(&branch), &branch)?;
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
        owned.check(vcs, rules, &origin(source), source)?;
    }
    Ok(())
}

/// `expect`: the environment's head the preview saw. `deployed_below`: per zip, whether the environment below
/// deployed it (for the warnings; the hub passes what its status knows).
pub fn merge_into_environment(
    vcs: &dyn Vcs,
    rules: &VsRules,
    request: &MergeRequest,
    expect: Option<&String>,
    deployed_below: &dyn Fn(&str, &str) -> bool,
    control: &RunControl<'_>,
) -> Result<MergeOutcome, VsError> {
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

/// One file to write or delete in a direct fix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileEdit {
    pub path: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommitRequest {
    pub branch: String,
    pub edits: Vec<FileEdit>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommitOutcome {
    pub branch: String,
    pub origin_head: String,
    pub commit: Option<String>,
    pub files: Vec<String>,
    pub pushed: bool,
}

/// A direct fix: environments and release branches anything, `main` only skeleton paths. The message may not
/// look like a merge (CI would deploy on it).
pub fn commit_files(
    vcs: &dyn Vcs,
    rules: &VsRules,
    request: &CommitRequest,
    expect: Option<&String>,
    control: &RunControl<'_>,
) -> Result<CommitOutcome, VsError> {
    vcs.fetch_all()?;
    let branch = request.branch.as_str();
    let is_main = branch == rules.settings.main;
    if !(is_main || rules.is_environment(branch) || rules.is_release_branch(branch)) {
        return Err(VsError::Refused(format!(
            "{branch} is not changed by hand here (environments, release branches and {} only)",
            rules.settings.main
        )));
    }
    let message = request.message.trim();
    if message.is_empty() {
        return Err(VsError::Invalid("a commit message is required".into()));
    }
    if VsRules::triggers_deploy(message) {
        return Err(VsError::Invalid(
            "the message may not start with `Merge branch` or `Auto-merge`: CI would deploy on it"
                .into(),
        ));
    }
    if request.edits.is_empty() {
        return Err(VsError::Invalid("no files to change".into()));
    }
    if is_main {
        rules.check_skeleton(request.edits.iter().map(|e| e.path.as_str()))?;
    }
    let origin_head = if control.dry_run {
        vcs.remote_head(branch)?
    } else {
        check_lease(vcs, branch, expect)?
    }
    .ok_or_else(|| VsError::Invalid(format!("{branch} does not exist on origin")))?;
    vcs.checkout_reset(branch, &origin(branch))?;
    let changes: Vec<FileChange> = request
        .edits
        .iter()
        .map(|e| FileChange {
            path: e.path.trim().to_owned(),
            content: e.content.clone().map(String::into_bytes),
        })
        .collect();
    let commit = vcs.commit_changes(&changes, message)?;
    let files = match &commit {
        Some(c) => changed_files(vcs, &origin_head, c)?,
        None => Vec::new(),
    };
    let mut outcome = CommitOutcome {
        branch: branch.to_owned(),
        origin_head: origin_head.clone(),
        commit: commit.clone(),
        files,
        pushed: false,
    };
    if control.dry_run || commit.is_none() {
        return Ok(outcome);
    }
    vcs.push(branch, PushMode::Expect(Some(origin_head)))?;
    (control.progress)(&format!("pushed {branch}"));
    outcome.pushed = true;
    Ok(outcome)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ResetRequest {
    pub branch: String,
    /// The commit the branch goes back to (an ancestor of its head).
    pub target: String,
    #[serde(default)]
    pub accept_losses: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ResetOutcome {
    pub branch: String,
    pub origin_head: String,
    pub target: CommitRef,
    /// The commits the reset drops, oldest first.
    pub losses: Vec<CommitRef>,
    pub pushed: bool,
}

/// A branch back to an earlier commit of its own (a force push with a lease). Zip branches are never touched
/// (the guard), and the target must be in the branch's history.
pub fn reset_branch(
    vcs: &dyn Vcs,
    rules: &VsRules,
    request: &ResetRequest,
    expect: Option<&String>,
    control: &RunControl<'_>,
) -> Result<ResetOutcome, VsError> {
    vcs.fetch_all()?;
    let branch = request.branch.as_str();
    rules.guard().check(branch)?;
    let origin_head = if control.dry_run {
        vcs.remote_head(branch)?
    } else {
        check_lease(vcs, branch, expect)?
    }
    .ok_or_else(|| VsError::Invalid(format!("{branch} does not exist on origin")))?;
    let target = vcs
        .rev_parse(request.target.trim())
        .map_err(|_| VsError::Invalid(format!("{} is not a commit", request.target)))?;
    if target == origin_head || !vcs.is_ancestor(&target, &origin_head)? {
        return Err(VsError::Invalid(format!(
            "{} is not an earlier commit of {branch}",
            &target[..target.len().min(8)]
        )));
    }
    let losses: Vec<CommitRef> = vcs
        .log(&target, &origin_head, false)?
        .iter()
        .map(CommitRef::from)
        .collect();
    let mut outcome = ResetOutcome {
        branch: branch.to_owned(),
        origin_head: origin_head.clone(),
        target: CommitRef::from(&vcs.commit(&target)?),
        losses,
        pushed: false,
    };
    if control.dry_run {
        return Ok(outcome);
    }
    if !request.accept_losses {
        return Err(VsError::Refused(format!(
            "resetting {branch} drops {} commit(s); tick to accept",
            outcome.losses.len()
        )));
    }
    vcs.checkout_reset(branch, &target)?;
    vcs.push(branch, PushMode::Expect(Some(origin_head)))?;
    (control.progress)(&format!("reset {branch} to {}", outcome.target.short));
    outcome.pushed = true;
    Ok(outcome)
}
