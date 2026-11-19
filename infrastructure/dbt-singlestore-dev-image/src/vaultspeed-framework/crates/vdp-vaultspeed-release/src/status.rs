use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use vdp_git::{Commit, RemoteBranch, Vcs};
use vdp_jira::IssueTracker;

use crate::zips::{ZipCache, ZipLookup, parse_zip_name};
use crate::{VsError, VsRules};

/// A commit as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommitRef {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub timestamp: i64,
}

impl From<&Commit> for CommitRef {
    fn from(commit: &Commit) -> Self {
        Self {
            hash: commit.hash.clone(),
            short: commit.short().to_owned(),
            subject: commit.subject().to_owned(),
            author: commit.author.clone(),
            timestamp: commit.timestamp,
        }
    }
}

impl From<&RemoteBranch> for CommitRef {
    fn from(branch: &RemoteBranch) -> Self {
        Self {
            hash: branch.head.clone(),
            short: branch.head.chars().take(8).collect(),
            subject: branch.subject.clone(),
            author: String::new(),
            timestamp: branch.timestamp,
        }
    }
}

/// One environment branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EnvironmentState {
    pub name: String,
    pub head: CommitRef,
    /// CI merges every zip branch into it itself.
    pub automatic: bool,
    /// May receive single zip branches (others only release branches).
    pub direct_zips: bool,
    /// Zips merged but not (yet) deployed: waiting for CI, or kept staged after a failure.
    pub staged: Vec<String>,
    /// PML scripts waiting for the next deploy.
    pub pml: Vec<String>,
}

/// A zip in one environment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ZipEnvironment {
    /// The environment's own commit that archived the zip (zips `main` carries do not count).
    pub deployed: Option<CommitRef>,
    pub staged: bool,
    /// `deploy_errors_*.csv` files the deploy left.
    pub errors: Vec<String>,
}

/// A Jira ticket, as far as a release needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TicketSummary {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub fix_versions: Vec<String>,
    /// Why Jira could not tell (the other fields are then empty).
    pub error: Option<String>,
}

/// One zip across the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ZipRow {
    pub name: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub number: u64,
    pub kind: String,
    /// `release/<name>` when VaultSpeed's branch still exists.
    pub branch: Option<String>,
    pub branch_head: Option<CommitRef>,
    pub tickets: Vec<TicketSummary>,
    pub vs_release: Option<String>,
    /// Per environment name.
    pub environments: BTreeMap<String, ZipEnvironment>,
    /// Release branches that contain it.
    pub releases: Vec<String>,
}

/// A release branch (`feature/<DROP>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ReleaseState {
    pub branch: String,
    pub head: CommitRef,
    pub zips: Vec<String>,
    pub pml: Vec<String>,
    /// Environments whose branch contains its head.
    pub merged_into: Vec<String>,
}

/// The whole picture the VaultSpeed pages start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "VsStatus"))]
pub struct Status {
    pub main: CommitRef,
    pub environments: Vec<EnvironmentState>,
    pub releases: Vec<ReleaseState>,
    /// Newest first.
    pub zips: Vec<ZipRow>,
}

/// Jira answers per ticket, kept until [`TicketCache::clear`].
#[derive(Debug, Default)]
pub struct TicketCache {
    tickets: Mutex<HashMap<String, TicketSummary>>,
}

impl TicketCache {
    /// Forgets every cached ticket.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// The summary of ticket `key`. A summary comes from the cache or from `tracker`; a lookup that fails, or no tracker, gives
    /// a summary whose `error` says why, and that is not cached.
    pub fn get(&self, tracker: Option<&dyn IssueTracker>, key: &str) -> TicketSummary {
        if let Some(found) = self.lock().get(key) {
            return found.clone();
        }
        let summary = match tracker.map(|t| t.ticket(key)) {
            Some(Ok(ticket)) => TicketSummary {
                key: key.to_owned(),
                summary: ticket.summary,
                status: ticket.status,
                fix_versions: ticket.fix_versions,
                error: None,
            },
            Some(Err(error)) => TicketSummary {
                key: key.to_owned(),
                summary: String::new(),
                status: String::new(),
                fix_versions: Vec::new(),
                error: Some(vdp_vaultspeed_domain::display_chain(&error)),
            },
            None => TicketSummary {
                key: key.to_owned(),
                summary: String::new(),
                status: String::new(),
                fix_versions: Vec::new(),
                error: Some("Jira is not configured".into()),
            },
        };
        // Errors are not kept: the next status asks again.
        if summary.error.is_none() {
            self.lock().insert(key.to_owned(), summary.clone());
        }
        summary
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, TicketSummary>> {
        self.tickets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn origin(branch: &str) -> String {
    format!("origin/{branch}")
}

fn zips_in(vcs: &dyn Vcs, rev: &str, dir: &str) -> Result<Vec<String>, VsError> {
    Ok(vcs
        .files_at(rev, dir)?
        .iter()
        .filter_map(|p| parse_zip_name(p).map(|(name, _, _)| name))
        .collect())
}

fn files_in(vcs: &dyn Vcs, rev: &str, dir: &str) -> Result<Vec<String>, VsError> {
    Ok(vcs
        .files_at(rev, dir)?
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned())
        .filter(|name| !name.starts_with('.'))
        .collect())
}

/// Per zip: the environment's last commit that archived it, unless a later one removed it again.
fn deployed_on(
    vcs: &dyn Vcs,
    rules: &VsRules,
    environment: &str,
) -> Result<BTreeMap<String, Option<Commit>>, VsError> {
    let archive = &rules.settings.archive_dir;
    let mut state: BTreeMap<String, Option<Commit>> = BTreeMap::new();
    for entry in vcs.path_log(&origin(&rules.settings.main), &origin(environment), archive)? {
        for file in &entry.files {
            let Some((name, _, _)) = parse_zip_name(&file.path) else {
                continue;
            };
            let present = matches!(file.status, 'A' | 'R' | 'M' | 'C');
            state.insert(name, present.then(|| entry.commit.clone()));
        }
    }
    Ok(state)
}

/// Collects the rows while walking the branches.
struct Inventory<'a> {
    vcs: &'a dyn Vcs,
    rules: &'a VsRules,
    rows: BTreeMap<String, ZipRow>,
    /// Where to read each zip from: (rev, path).
    source_of: BTreeMap<String, (String, String)>,
}

impl Inventory<'_> {
    /// The row of `name`, created on first sight (`None`: not a zip name).
    fn row(&mut self, name: &str) -> Option<&mut ZipRow> {
        if !self.rows.contains_key(name) {
            let (name, number, kind) = parse_zip_name(name)?;
            self.rows.insert(
                name.clone(),
                ZipRow {
                    name,
                    number,
                    kind,
                    branch: None,
                    branch_head: None,
                    tickets: Vec::new(),
                    vs_release: None,
                    environments: BTreeMap::new(),
                    releases: Vec::new(),
                },
            );
        }
        self.rows.get_mut(name)
    }

    /// Zip branches (VaultSpeed's agent): the zip each one stages.
    fn zip_branches(&mut self) -> Result<(), VsError> {
        let s = &self.rules.settings;
        for branch in self.vcs.remote_branches(&s.zip_prefix)? {
            let rev = origin(&branch.name);
            for zip in zips_in(self.vcs, &rev, &s.staging_dir)? {
                let path = format!("{}/{zip}.zip", s.staging_dir);
                if let Some(row) = self.row(&zip) {
                    row.branch = Some(branch.name.clone());
                    row.branch_head = Some(CommitRef::from(&branch));
                    self.source_of.insert(zip, (rev.clone(), path));
                }
            }
        }
        Ok(())
    }

    /// One environment: what it deployed (its own commits), what is staged, which deploys left errors.
    fn environment(&mut self, env: &str) -> Result<Option<EnvironmentState>, VsError> {
        let s = &self.rules.settings;
        if self.vcs.remote_head(env)?.is_none() {
            tracing::warn!(environment = %env, "no such branch on origin");
            return Ok(None);
        }
        let rev = origin(env);
        let head = self.vcs.commit(&rev)?;
        let staged = zips_in(self.vcs, &rev, &s.staging_dir)?;
        let pml = files_in(self.vcs, &rev, &s.pml_dir)?;
        let errors: Vec<String> = self
            .vcs
            .files_at(&rev, &s.logs_dir)?
            .into_iter()
            .filter(|p| p.contains("/deploy_errors_"))
            .collect();
        let archive_dir = s.archive_dir.clone();
        let logs_prefix = format!("{}/", s.logs_dir);
        let state = EnvironmentState {
            name: env.to_owned(),
            head: CommitRef::from(&head),
            automatic: s.auto_environments.iter().any(|e| e == env),
            direct_zips: s.direct_merge_environments.iter().any(|e| e == env),
            staged: staged.clone(),
            pml,
        };
        for (zip, commit) in deployed_on(self.vcs, self.rules, env)? {
            let deployed = commit.as_ref().map(CommitRef::from);
            let Some(row) = self.row(&zip) else { continue };
            row.environments.entry(env.to_owned()).or_default().deployed = deployed;
            if commit.is_some() && !self.source_of.contains_key(&zip) {
                let path = format!("{archive_dir}/{zip}.zip");
                self.source_of.insert(zip, (rev.clone(), path));
            }
        }
        for zip in &staged {
            if let Some(row) = self.row(zip) {
                row.environments.entry(env.to_owned()).or_default().staged = true;
            }
        }
        for path in errors {
            let zip = path
                .strip_prefix(&logs_prefix)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or_default()
                .to_owned();
            if let Some(row) = self.rows.get_mut(&zip) {
                row.environments
                    .entry(env.to_owned())
                    .or_default()
                    .errors
                    .push(path);
            }
        }
        Ok(Some(state))
    }

    /// Release branches (`feature/<DROP>`): their zips and PML, and where they were merged.
    fn releases(&mut self) -> Result<Vec<ReleaseState>, VsError> {
        let s = &self.rules.settings;
        let mut releases = Vec::new();
        for branch in self.vcs.remote_branches(&s.release_prefix)? {
            let rev = origin(&branch.name);
            let zips = zips_in(self.vcs, &rev, &s.staging_dir)?;
            let merged_into = s
                .environments
                .iter()
                .filter(|env| self.vcs.is_ancestor(&rev, &origin(env)).unwrap_or(false))
                .cloned()
                .collect();
            for zip in &zips {
                if let Some(row) = self.rows.get_mut(zip) {
                    row.releases.push(branch.name.clone());
                }
            }
            releases.push(ReleaseState {
                branch: branch.name.clone(),
                head: CommitRef::from(&branch),
                pml: files_in(self.vcs, &rev, &s.pml_dir)?,
                zips,
                merged_into,
            });
        }
        Ok(releases)
    }

    /// What each zip says (cached per blob) and what Jira says about its tickets.
    fn annotate(
        &mut self,
        zips: &ZipCache,
        tickets: &TicketCache,
        tracker: Option<&dyn IssueTracker>,
    ) {
        let prefix = &self.rules.settings.ticket_prefix;
        for (zip, (rev, path)) in &self.source_of {
            let Some(row) = self.rows.get_mut(zip) else {
                continue;
            };
            match zips.get(
                self.vcs,
                ZipLookup {
                    rev,
                    path,
                    ticket_prefix: prefix,
                },
            ) {
                Ok(info) => {
                    row.vs_release = info.vs_release;
                    row.tickets = info
                        .tickets
                        .iter()
                        .map(|key| tickets.get(tracker, key))
                        .collect();
                }
                Err(error) => tracing::warn!(
                    %zip,
                    error = %vdp_vaultspeed_domain::display_chain(&error),
                    "cannot read the zip"
                ),
            }
        }
    }
}

/// Inputs for [`status`].
#[derive(Clone, Copy)]
pub struct StatusInputs<'a> {
    pub rules: &'a VsRules,
    pub zips: &'a ZipCache,
    pub tickets: &'a TicketCache,
    pub tracker: Option<&'a dyn IssueTracker>,
}

impl std::fmt::Debug for StatusInputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusInputs")
            .field("rules", &self.rules)
            .field("zips", &self.zips)
            .field("tickets", &self.tickets)
            .field("tracker", &self.tracker.is_some())
            .finish()
    }
}

/// Everything about the zips, environments and release branches, from the remote-tracking refs (fetch first).
///
/// # Errors
///
/// [`VsError::Git`] when a remote-tracking ref or its history cannot be read; [`VsError::Zip`] when a zip cannot be read.
pub fn status(vcs: &dyn Vcs, inputs: StatusInputs<'_>) -> Result<Status, VsError> {
    let StatusInputs {
        rules,
        zips,
        tickets,
        tracker,
    } = inputs;
    let main = vcs.commit(&origin(&rules.settings.main))?;
    let mut inventory = Inventory {
        vcs,
        rules,
        rows: BTreeMap::new(),
        source_of: BTreeMap::new(),
    };
    inventory.zip_branches()?;
    let mut environments = Vec::new();
    for env in rules.environments() {
        environments.extend(inventory.environment(env)?);
    }
    let releases = inventory.releases()?;
    inventory.annotate(zips, tickets, tracker);
    let mut rows: Vec<ZipRow> = inventory.rows.into_values().collect();
    rows.sort_by(|a, b| b.number.cmp(&a.number).then_with(|| a.name.cmp(&b.name)));
    Ok(Status {
        main: CommitRef::from(&main),
        environments,
        releases,
        zips: rows,
    })
}

/// Zips whose tickets carry `fix_version` (proposed for a release of that DROP).
pub fn proposed_for(status: &Status, fix_version: &str) -> BTreeSet<String> {
    status
        .zips
        .iter()
        .filter(|z| {
            z.tickets
                .iter()
                .any(|t| t.fix_versions.iter().any(|f| f == fix_version))
        })
        .map(|z| z.name.clone())
        .collect()
}
