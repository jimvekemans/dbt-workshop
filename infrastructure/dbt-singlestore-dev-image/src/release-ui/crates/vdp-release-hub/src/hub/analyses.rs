//! Analysis creation, listing, staleness checks, and ticket selection on [`Hub`].

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use vdp_git::Vcs;
use vdp_jira::FixVersion;
use vdp_release::{
    Analyser, Analysis, AnalysisKind, AnalysisRequest, Progress, ScopeAction, ScopeCheck,
    check_scope, remote_ref,
};

use super::{Hub, Stored, analysis_id, count, lock};
use crate::api::{AnalysisCreate, AnalysisSummary, AnalysisView, SelectionUpdate};
use crate::error::HubError;
use crate::jobs::{Job, JobKind, JobLog, JobResult, now_ms};

impl Hub {
    fn request_for(&self, create: &AnalysisCreate) -> Result<(AnalysisRequest, String), HubError> {
        let release = &self.config.release;
        let given = |v: &Option<String>| v.clone().filter(|s| !s.trim().is_empty());
        let source = remote_ref(&given(&create.source).unwrap_or_else(|| release.from.clone()));
        let (compare, branch) = match create.kind {
            AnalysisKind::Release => (
                given(&create.compare).unwrap_or_else(|| release.to.clone()),
                None,
            ),
            AnalysisKind::Patch => {
                let branch = given(&create.compare).ok_or_else(|| {
                    HubError::BadRequest("a patch needs the release branch (compare)".into())
                })?;
                let target = self.rules.release_target(&branch)?;
                check_scope(
                    &self.git(),
                    &ScopeCheck {
                        rules: &self.rules,
                        branch: &target,
                        action: ScopeAction::Patch,
                    },
                )?;
                (branch.clone(), Some(target))
            }
        };
        let fix_version = match given(&create.fix_version) {
            Some(v) => v,
            None => branch
                .as_deref()
                .and_then(|b| b.strip_prefix(&release.branch_prefix))
                .map(str::to_owned)
                .ok_or_else(|| HubError::BadRequest("fix_version is required".into()))?,
        };
        fix_version
            .parse::<FixVersion>()
            .map_err(|e| HubError::BadRequest(format!("fix_version: {e}")))?;
        let branch = branch.unwrap_or_else(|| format!("{}{fix_version}", release.branch_prefix));
        Ok((
            AnalysisRequest {
                kind: create.kind,
                source,
                compare: remote_ref(&compare),
                fix_version,
            },
            branch,
        ))
    }

    /// Starts a job that analyses the refs of `create` and stores the result with its default selection.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when there is no clone yet; [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started; [`HubError::BadRequest`] when a patch has no release branch, the branch is not a release branch or is
    /// outside the newest two, or the fix version is missing or invalid. The analysis itself runs in the job.
    pub fn create_analysis(self: &Arc<Self>, create: &AnalysisCreate) -> Result<Job, HubError> {
        self.require_clone()?;
        let (request, branch) = self.request_for(create)?;
        let keep_empty = create.keep_empty.unwrap_or(false);
        let title = format!(
            "analyse {} {} vs {} for {}",
            match request.kind {
                AnalysisKind::Release => "release",
                AnalysisKind::Patch => "patch",
            },
            request.source,
            request.compare,
            request.fix_version
        );
        let hub = Arc::clone(self);
        self.jobs.start(JobKind::Analysis, title, move |log| {
            let analysis = hub.analyse(&request, keep_empty, log)?;
            let n = hub.next_analysis.fetch_add(1, Ordering::Relaxed) + 1;
            let selection = analysis.default_selection();
            log.info(format!(
                "{} tickets, {} selected by default, {} uncoupled commits",
                analysis.tickets.len(),
                selection.len(),
                analysis.uncoupled.len()
            ));
            let mut store = lock(&hub.analyses);
            store.insert(
                n,
                Stored {
                    created: now_ms(),
                    branch,
                    analysis,
                    selection,
                    version: 0,
                },
            );
            hub.save_analyses(&store);
            Ok(JobResult::Analysis {
                analysis_id: analysis_id(n),
            })
        })
    }

    fn analyse(
        &self,
        request: &AnalysisRequest,
        keep_empty: bool,
        log: &JobLog,
    ) -> Result<Analysis, String> {
        let git = self.git();
        log.info("fetching origin");
        // An analysis of stale refs would be reviewed and released as if it were current: fail instead.
        git.fetch_all().map_err(|error| {
            format!(
                "fetch from origin failed, so the analysis would use stale refs: {}",
                vdp_core::display_chain(&error)
            )
        })?;
        let tracker = match &self.tracker {
            Ok(tracker) => Some(tracker.as_ref()),
            Err(why) => {
                log.warn(format!("Jira is not available: {why}"));
                None
            }
        };
        let progress = |p: Progress| match p {
            Progress::Step(message) => log.info(message),
            Progress::Jira(done, total, key) => log.info(format!("Jira {done}/{total}: {key}")),
        };
        let analysis = Analyser {
            vcs: &git,
            rules: &self.rules,
            tracker,
            jira_base_url: self.config.jira_base_url.clone(),
            ticket_prefix: self.config.release.ticket_prefix.clone(),
            excluded: self.config.excluded_fix_versions.clone(),
            keep_empty,
            progress: &progress,
        }
        .analyse(request)
        .map_err(|e| vdp_core::display_chain(&e))?;
        let errors = analysis.jira_errors();
        if tracker.is_some() && !errors.is_empty() {
            log.warn(format!(
                "{} tickets without Jira data (first: {}: {})",
                errors.len(),
                errors[0].0,
                errors[0].1
            ));
        }
        Ok(analysis)
    }

    pub(super) fn stored(&self, id: &str) -> Result<Stored, HubError> {
        id.strip_prefix('a')
            .and_then(|n| n.parse::<u64>().ok())
            .and_then(|n| lock(&self.analyses).get(&n).cloned())
            .ok_or_else(|| HubError::NotFound(format!("analysis {id}")))
    }

    pub(super) fn is_stale(&self, analysis: &Analysis) -> bool {
        let git = self.git();
        let moved = |rev: &str, head: &str| git.rev_parse(rev).map_or(true, |h| h != head);
        moved(&analysis.request.source, &analysis.source_head)
            || moved(&analysis.request.compare, &analysis.compare_head)
    }

    fn view(&self, id: &str, stored: Stored) -> AnalysisView {
        AnalysisView {
            id: id.to_owned(),
            created: stored.created,
            stale: self.is_stale(&stored.analysis),
            branch: stored.branch,
            selection: stored.selection.into_iter().collect(),
            analysis: stored.analysis,
            version: stored.version,
        }
    }

    /// The stored analysis `id`, with its selection.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such analysis.
    pub fn analysis(&self, id: &str) -> Result<AnalysisView, HubError> {
        let stored = self.stored(id)?;
        Ok(self.view(id, stored))
    }

    /// Summaries of the stored analyses, newest first.
    pub fn analyses(&self) -> Vec<AnalysisSummary> {
        lock(&self.analyses)
            .iter()
            .rev()
            .map(|(n, s)| AnalysisSummary {
                id: analysis_id(*n),
                created: s.created,
                kind: s.analysis.request.kind,
                fix_version: s.analysis.request.fix_version.clone(),
                branch: s.branch.clone(),
                tickets: count(s.analysis.tickets.len()),
                selected: count(s.selection.len()),
            })
            .collect()
    }

    /// Replaces the selection of analysis `id`; the version of the board is bumped.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such analysis; [`HubError::Conflict`] when `update.version` is not the current
    /// version (someone else edited it); [`HubError::BadRequest`] when the selection has an unknown or unselectable ticket.
    pub fn set_selection(
        &self,
        id: &str,
        update: &SelectionUpdate,
    ) -> Result<AnalysisView, HubError> {
        let selection: BTreeSet<String> = update.selection.iter().cloned().collect();
        let stored = {
            let mut analyses = lock(&self.analyses);
            let stored = id
                .strip_prefix('a')
                .and_then(|n| n.parse::<u64>().ok())
                .and_then(|n| analyses.get_mut(&n))
                .ok_or_else(|| HubError::NotFound(format!("analysis {id}")))?;
            if let Some(version) = update.version
                && version != stored.version
            {
                return Err(HubError::Conflict(
                    "the board changed since you loaded it (someone else edited it): reload it"
                        .into(),
                ));
            }
            stored.analysis.validate_selection(&selection)?;
            stored.selection = selection;
            stored.version += 1;
            let copy = stored.clone();
            self.save_analyses(&analyses);
            copy
        };
        Ok(self.view(id, stored))
    }
}
