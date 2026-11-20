//! Git jobs: one at a time (concurrent git on one work tree is unsafe), each with a step log read by cursor.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use vdp_release::{ConflictReport, Outcome, Promoted, Resolution};

use crate::api::RepoStatus;
use crate::error::HubError;

/// Milliseconds since the epoch.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// What a job does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Clone,
    Sync,
    Analysis,
    Release,
    Patch,
    Promotion,
    Repair,
    /// A job of a hub extension, under the extension's own name for it (e.g. `vs_release`).
    #[serde(untagged)]
    Other(String),
}

/// Where a job is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    /// Stopped on a conflict until the release manager decides (`pending`).
    Waiting,
    /// The hub restarted while it ran: check the clone's health (repair it if needed).
    Interrupted,
    Done,
    Failed,
}

/// The severity of a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// One log line of a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JobStep {
    /// 1, 2, ...: poll with `?after=<seq>` for the lines after it.
    pub seq: u32,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub ts: i64,
    pub level: Level,
    pub message: String,
}

/// What a finished job produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobResult {
    Repo {
        repo: RepoStatus,
    },
    Analysis {
        analysis_id: String,
    },
    Release {
        outcome: Outcome,
    },
    Patch {
        outcome: Outcome,
    },
    Promotion {
        promoted: Promoted,
    },
    /// What a job of a hub extension produced: its own document, which carries its own `type`.
    #[serde(untagged)]
    Extension(
        #[cfg_attr(
            feature = "ts",
            ts(type = "{ type: string } & { [key: string]: unknown }")
        )]
        serde_json::Value,
    ),
}

/// A job as the API shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub title: String,
    pub status: JobStatus,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub started: i64,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub finished: Option<i64>,
    /// The steps after the requested cursor (all of them without one).
    pub steps: Vec<JobStep>,
    /// The highest `seq` so far: the next cursor.
    pub last_seq: u32,
    pub result: Option<JobResult>,
    pub error: Option<String>,
    /// The conflict waiting for a decision.
    pub pending: Option<ConflictReport>,
}

#[derive(Debug, Default)]
struct Inner {
    jobs: BTreeMap<u64, Job>,
    next: u64,
    active: Option<u64>,
    /// Where the decision for a waiting job goes.
    waiting: BTreeMap<u64, Sender<Resolution>>,
}

/// The jobs of this process (in memory).
#[derive(Debug, Default)]
pub struct Jobs {
    inner: Mutex<Inner>,
    /// Finished jobs kept (oldest dropped first).
    keep: usize,
    /// Where the job history is kept across restarts.
    state_dir: Option<std::path::PathBuf>,
}

/// Lets a running job write its log.
#[derive(Debug, Clone)]
pub struct JobLog {
    jobs: Arc<Jobs>,
    id: u64,
}

impl JobLog {
    /// Appends a line to the job's log (and to the process log).
    pub fn log(&self, level: Level, message: impl Into<String>) {
        let message = message.into();
        match level {
            Level::Info => tracing::info!(job = self.id, "{message}"),
            Level::Warn => tracing::warn!(job = self.id, "{message}"),
            Level::Error => tracing::error!(job = self.id, "{message}"),
        }
        let mut inner = self.jobs.lock();
        if let Some(job) = inner.jobs.get_mut(&self.id) {
            job.last_seq += 1;
            let seq = job.last_seq;
            job.steps.push(JobStep {
                seq,
                ts: now_ms(),
                level,
                message,
            });
        }
    }

    /// Appends an informational line.
    pub fn info(&self, message: impl Into<String>) {
        self.log(Level::Info, message);
    }

    /// Puts the conflict to the release manager and blocks until they decide (or `timeout` passes: then the
    /// run is aborted, nothing is pushed).
    pub fn wait_for(&self, report: ConflictReport, timeout: Duration) -> Resolution {
        let (send, receive) = channel();
        {
            let mut inner = self.jobs.lock();
            inner.waiting.insert(self.id, send);
            if let Some(job) = inner.jobs.get_mut(&self.id) {
                job.status = JobStatus::Waiting;
                job.pending = Some(report);
            }
        }
        self.jobs.save();
        self.warn("conflict: waiting for a decision in the release UI");
        let decision = receive.recv_timeout(timeout);
        {
            let mut inner = self.jobs.lock();
            inner.waiting.remove(&self.id);
            if let Some(job) = inner.jobs.get_mut(&self.id) {
                job.status = JobStatus::Running;
                job.pending = None;
            }
        }
        match decision {
            Ok(resolution) => {
                self.info(format!("decision: {:?}", resolution.action).to_lowercase());
                resolution
            }
            Err(_) => {
                self.warn(format!(
                    "no decision within {} min: aborting (nothing is pushed)",
                    timeout.as_secs() / 60
                ));
                Resolution::abort()
            }
        }
    }

    /// Appends a warning.
    pub fn warn(&self, message: impl Into<String>) {
        self.log(Level::Warn, message);
    }
}

fn job_id(n: u64) -> String {
    format!("j{n}")
}

fn parse_id(id: &str) -> Option<u64> {
    id.strip_prefix('j')?.parse().ok()
}

impl Jobs {
    /// Creates the registry. It keeps the latest `keep` jobs (at least one) and, with `state_dir`, saves them there and
    /// restores them (a job that was running becomes interrupted).
    pub fn new(keep: usize, state_dir: Option<std::path::PathBuf>) -> Self {
        let mut inner = Inner::default();
        if let Some(saved) = state_dir
            .as_deref()
            .and_then(|dir| crate::hub::load_json::<Vec<Job>>(dir, "jobs.json"))
        {
            for mut job in saved {
                let Some(n) = parse_id(&job.id) else { continue };
                if matches!(job.status, JobStatus::Running | JobStatus::Waiting) {
                    job.status = JobStatus::Interrupted;
                    job.pending = None;
                    job.error = Some(
                        "interrupted by a restart of the release hub; check the clone's health"
                            .into(),
                    );
                }
                inner.next = inner.next.max(n);
                inner.jobs.insert(n, job);
            }
        }
        Self {
            inner: Mutex::new(inner),
            keep: keep.max(1),
            state_dir,
        }
    }

    /// Saves the history (call with the lock released).
    fn save(&self) {
        if let Some(dir) = &self.state_dir {
            let jobs: Vec<Job> = self.lock().jobs.values().cloned().collect();
            crate::hub::save_json(dir, "jobs.json", &jobs);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts `work` on its own thread, unless a job is running.
    ///
    /// # Errors
    ///
    /// [`HubError::Busy`] when another job is running; [`HubError::Internal`] when the job's thread cannot be started (the
    /// job is then not kept and the next one can start).
    pub fn start<F>(
        self: &Arc<Self>,
        kind: JobKind,
        title: impl Into<String>,
        work: F,
    ) -> Result<Job, HubError>
    where
        F: FnOnce(&JobLog) -> Result<JobResult, String> + Send + 'static,
    {
        self.start_on(kind, title, work, |name, run| {
            std::thread::Builder::new().name(name).spawn(run).map(drop)
        })
    }

    /// [`Jobs::start`] with the way the thread is started given (`spawn` gets the thread's name and what it runs), so that
    /// a test can make the start fail.
    fn start_on<F, S>(
        self: &Arc<Self>,
        kind: JobKind,
        title: impl Into<String>,
        work: F,
        spawn: S,
    ) -> Result<Job, HubError>
    where
        F: FnOnce(&JobLog) -> Result<JobResult, String> + Send + 'static,
        S: FnOnce(String, Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
    {
        let job = {
            let mut inner = self.lock();
            if let Some(active) = inner.active.and_then(|id| inner.jobs.get(&id)) {
                return Err(HubError::Busy(format!(
                    "{} is still running ({})",
                    active.id, active.title
                )));
            }
            inner.next += 1;
            let n = inner.next;
            let job = Job {
                id: job_id(n),
                kind,
                title: title.into(),
                status: JobStatus::Running,
                started: now_ms(),
                finished: None,
                steps: Vec::new(),
                last_seq: 0,
                result: None,
                error: None,
                pending: None,
            };
            inner.jobs.insert(n, job.clone());
            inner.active = Some(n);
            self.prune(&mut inner);
            (n, job)
        };
        self.save();
        let (n, view) = job;
        let log = JobLog {
            jobs: Arc::clone(self),
            id: n,
        };
        let run = move || {
            let result = catch_unwind(AssertUnwindSafe(|| work(&log)))
                .unwrap_or_else(|_| Err("internal error (the job panicked)".into()));
            if let Err(error) = &result {
                log.log(Level::Error, error.clone());
            }
            let mut inner = log.jobs.lock();
            if let Some(job) = inner.jobs.get_mut(&n) {
                job.finished = Some(now_ms());
                match result {
                    Ok(value) => {
                        job.status = JobStatus::Done;
                        job.result = Some(value);
                    }
                    Err(error) => {
                        job.status = JobStatus::Failed;
                        job.error = Some(error);
                    }
                }
            }
            inner.active = None;
            drop(inner);
            log.jobs.save();
        };
        if let Err(error) = spawn(format!("job-{n}"), Box::new(run)) {
            // The job never ran: take it back, or the hub would answer `busy` until it is restarted.
            {
                let mut inner = self.lock();
                inner.jobs.remove(&n);
                if inner.active == Some(n) {
                    inner.active = None;
                }
            }
            self.save();
            return Err(HubError::Internal(format!(
                "could not start the job: {error}"
            )));
        }
        Ok(view)
    }

    fn prune(&self, inner: &mut Inner) {
        while inner.jobs.len() > self.keep {
            let oldest = inner
                .jobs
                .iter()
                .find(|(id, _)| Some(**id) != inner.active)
                .map(|(id, _)| *id);
            match oldest {
                Some(id) => inner.jobs.remove(&id),
                None => break,
            };
        }
    }

    /// The job with the steps after `after`.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such job.
    pub fn get(&self, id: &str, after: u32) -> Result<Job, HubError> {
        let inner = self.lock();
        let job = parse_id(id)
            .and_then(|n| inner.jobs.get(&n))
            .ok_or_else(|| HubError::NotFound(format!("job {id}")))?;
        let mut view = job.clone();
        view.steps.retain(|s| s.seq > after);
        Ok(view)
    }

    /// Hands the release manager's decision to a waiting job.
    ///
    /// # Errors
    ///
    /// [`HubError::NotFound`] when there is no such job; [`HubError::Conflict`] when it is not waiting for a decision (or
    /// stopped waiting); [`HubError::BadRequest`] when `resolution` names a file that is not in the conflict.
    pub fn resolve(&self, id: &str, resolution: Resolution) -> Result<(), HubError> {
        let mut inner = self.lock();
        let n = parse_id(id).ok_or_else(|| HubError::NotFound(format!("job {id}")))?;
        let job = inner
            .jobs
            .get(&n)
            .ok_or_else(|| HubError::NotFound(format!("job {id}")))?;
        let Some(pending) = &job.pending else {
            return Err(HubError::Conflict(format!(
                "{id} is not waiting for a decision"
            )));
        };
        if let Some(unknown) = resolution
            .choices
            .keys()
            .find(|path| !pending.files.iter().any(|f| &f.path == *path))
        {
            return Err(HubError::BadRequest(format!(
                "{unknown} is not a conflicting file"
            )));
        }
        let sender = inner
            .waiting
            .remove(&n)
            .ok_or_else(|| HubError::Conflict(format!("{id} is not waiting for a decision")))?;
        sender
            .send(resolution)
            .map_err(|_| HubError::Conflict(format!("{id} stopped waiting")))
    }

    /// The conflict a job waits on.
    ///
    /// # Errors
    ///
    /// [`HubError::Conflict`] when the job is not waiting for a decision.
    pub fn pending(&self, id: &str) -> Result<ConflictReport, HubError> {
        let inner = self.lock();
        parse_id(id)
            .and_then(|n| inner.jobs.get(&n))
            .and_then(|job| job.pending.clone())
            .ok_or_else(|| HubError::Conflict(format!("{id} is not waiting for a decision")))
    }

    /// The running job, without its steps.
    pub fn active(&self) -> Option<Job> {
        let inner = self.lock();
        inner
            .active
            .and_then(|id| inner.jobs.get(&id))
            .map(|job| Job {
                steps: Vec::new(),
                ..job.clone()
            })
    }

    /// Every kept job, newest first, without steps.
    pub fn list(&self) -> Vec<Job> {
        self.lock()
            .jobs
            .values()
            .rev()
            .map(|job| Job {
                steps: Vec::new(),
                ..job.clone()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn wait(jobs: &Jobs, id: &str) -> Job {
        for _ in 0..200 {
            let job = jobs.get(id, 0).unwrap();
            if job.status != JobStatus::Running {
                return job;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("job {id} did not finish");
    }

    #[test]
    fn one_job_at_a_time_with_a_cursor() {
        let jobs = Arc::new(Jobs::new(10, None));
        let (release, gate) = mpsc::channel::<()>();
        let first = jobs
            .start(JobKind::Sync, "sync", move |log| {
                log.info("one");
                gate.recv().unwrap();
                log.warn("two");
                Ok(JobResult::Analysis {
                    analysis_id: "a1".into(),
                })
            })
            .unwrap();
        let busy = jobs.start(JobKind::Sync, "again", |_| Err("no".into()));
        assert!(matches!(busy, Err(HubError::Busy(_))), "{busy:?}");
        assert_eq!(jobs.active().unwrap().id, first.id);
        release.send(()).unwrap();
        let done = wait(&jobs, &first.id);
        assert_eq!(done.status, JobStatus::Done);
        assert_eq!(done.last_seq, 2);
        let after = jobs.get(&first.id, 1).unwrap();
        assert_eq!(after.steps.len(), 1);
        assert_eq!(after.steps[0].message, "two");
        assert!(jobs.active().is_none());
    }

    #[test]
    fn a_thread_that_cannot_start_does_not_leave_the_hub_busy() {
        let jobs = Arc::new(Jobs::new(10, None));
        let refused = jobs.start_on(
            JobKind::Sync,
            "sync",
            |_| {
                Ok(JobResult::Analysis {
                    analysis_id: "a1".into(),
                })
            },
            |_, _| Err(std::io::Error::other("no more threads")),
        );
        assert!(
            matches!(&refused, Err(HubError::Internal(m)) if m.contains("could not start the job: no more threads")),
            "{refused:?}"
        );
        // Nothing is left of it: not active, not in the history.
        assert!(jobs.active().is_none());
        assert!(jobs.list().is_empty());
        // The next job starts (not `busy`) and runs.
        let next = jobs
            .start(JobKind::Sync, "again", |_| {
                Ok(JobResult::Analysis {
                    analysis_id: "a2".into(),
                })
            })
            .unwrap();
        assert_eq!(wait(&jobs, &next.id).status, JobStatus::Done);
    }

    #[test]
    fn failures_and_panics_end_the_job() {
        let jobs = Arc::new(Jobs::new(1, None));
        let failed = jobs
            .start(JobKind::Clone, "clone", |_| Err("nope".into()))
            .unwrap();
        let failed = wait(&jobs, &failed.id);
        assert_eq!(failed.status, JobStatus::Failed);
        assert_eq!(failed.error.as_deref(), Some("nope"));
        let panicked = jobs
            .start(JobKind::Clone, "clone", |_| panic!("boom"))
            .unwrap();
        assert_eq!(wait(&jobs, &panicked.id).status, JobStatus::Failed);
        // Only one finished job is kept.
        assert!(jobs.get(&failed.id, 0).is_err());
        assert!(matches!(jobs.get("x", 0), Err(HubError::NotFound(_))));
    }
}
