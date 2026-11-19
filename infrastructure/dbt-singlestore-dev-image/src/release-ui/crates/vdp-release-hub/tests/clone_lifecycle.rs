//! End-to-end clone lifecycle tests for `Hub` against a local bare repository (no network).

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use vdp_git::{GitCli, Vcs};
use vdp_notify::{Announcement, Delivery, Mailer, NotifyError};
use vdp_release::ReleaseSettings;
use vdp_release_hub::api::{CloneRequest, GitIdentity};
use vdp_release_hub::jobs::{Job, JobResult, JobStatus};
use vdp_release_hub::{Hub, HubConfig, HubError, HubServices};

#[derive(Debug)]
struct SilentMailer;

impl Mailer for SilentMailer {
    fn readiness(&self) -> Result<(), String> {
        Err("no mail in tests".into())
    }

    fn send(&self, _: &Announcement, _: &[String]) -> Result<Delivery, NotifyError> {
        unreachable!("tests send nothing")
    }
}

fn git_run(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Seed", "-c", "user.email=seed@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn wait_done(hub: &Hub, id: &str) -> Job {
    for _ in 0..500 {
        let job = hub.job(id, 0).unwrap();
        if job.status != JobStatus::Running {
            assert_eq!(
                job.status,
                JobStatus::Done,
                "job {id} failed: {:?}",
                job.error
            );
            return job;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("job {id} timed out");
}

fn seed_bare_remote(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, String) {
    let origin = root.join("origin.git");
    let work = root.join("seed");
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&origin)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        Command::new("git")
            .args(["clone", origin.to_str().unwrap(), work.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
    git_run(&work, &["checkout", "-b", "main"]);
    std::fs::write(work.join("README.md"), "main1\n").unwrap();
    git_run(&work, &["add", "README.md"]);
    git_run(&work, &["commit", "-m", "main commit"]);
    git_run(&work, &["push", "-u", "origin", "main"]);

    git_run(&work, &["checkout", "-b", "dev"]);
    std::fs::write(work.join("README.md"), "dev1\n").unwrap();
    git_run(&work, &["commit", "-am", "dev commit 1"]);
    git_run(&work, &["push", "-u", "origin", "dev"]);
    let dev_sha1 = git_run(&work, &["rev-parse", "HEAD"]);
    (origin, work, dev_sha1)
}

#[test]
fn hub_clone_lifecycle_exercises_identity_protected_branch_repair_sync_and_reclone() {
    let dir = tempfile::tempdir().unwrap();
    let (origin, work, dev_sha1) = seed_bare_remote(dir.path());
    let clone_dir = dir.path().join("hub_clone");
    let hub = Hub::new(
        HubConfig {
            repo_dir: clone_dir.clone(),
            repo_url: origin.to_string_lossy().into_owned(),
            release: ReleaseSettings::default(),
            jira_base_url: String::new(),
            excluded_fix_versions: Vec::new(),
            default_recipients: Vec::new(),
            keep_jobs: 10,
            git_timeout: None,
            conflict_timeout: Duration::from_secs(1),
            state_dir: None,
            gitlab: None,
        },
        HubServices {
            tracker: Err("no Jira".into()),
            mailer: Arc::new(SilentMailer),
            validator: None,
            extensions: Vec::new(),
        },
    );

    assert!(!hub.repo_status().cloned);
    let expected_not_cloned = format!(
        "{} is not cloned yet: clone the repository first",
        clone_dir.display()
    );
    for err in [hub.sync().unwrap_err(), hub.repair().unwrap_err()] {
        assert!(
            matches!(err, HubError::Conflict(ref msg) if msg == &expected_not_cloned),
            "{err:?}"
        );
    }

    let cloned = wait_done(
        &hub,
        &hub.clone_repo(&CloneRequest { url: None }).unwrap().id,
    );
    assert!(matches!(cloned.result, Some(JobResult::Repo { .. })));
    let st = hub.repo_status();
    assert!(st.cloned);
    assert_eq!(st.head.as_deref(), Some(dev_sha1.as_str()));
    assert_eq!(st.health, vec![]);

    let again = hub.clone_repo(&CloneRequest { url: None }).unwrap_err();
    assert!(
        matches!(again, HubError::Conflict(ref m) if m == "the repository is already cloned: sync it instead")
    );

    let id = GitIdentity {
        name: "Release Manager".into(),
        email: "rm@example.com".into(),
    };
    hub.set_identity(&id).unwrap();
    assert_eq!(hub.status(None).identity, Some(id.clone()));
    let actor = hub.actor(None, true).unwrap();
    assert_eq!(
        actor.identity.as_ref().map(|i| i.email.as_str()),
        Some("rm@example.com")
    );

    git_run(&clone_dir, &["checkout", "-b", "dev"]);
    let unhealthy = hub.repo_status();
    assert!(
        unhealthy.health.iter().any(|p| p.code == "local_branches"
            && p.detail
                == "a local dev exists: the hub keeps none (nothing may ever be committed on it)"),
        "unexpected health: {:?}",
        unhealthy.health
    );

    wait_done(&hub, &hub.repair().unwrap().id);
    let repaired = hub.repo_status();
    assert_eq!(repaired.health, vec![]);
    assert_eq!(repaired.head.as_deref(), Some(dev_sha1.as_str()));

    std::fs::write(work.join("README.md"), "dev2\n").unwrap();
    git_run(&work, &["commit", "-am", "dev commit 2"]);
    git_run(&work, &["push", "origin", "dev"]);
    let dev_sha2 = git_run(&work, &["rev-parse", "HEAD"]);

    wait_done(&hub, &hub.sync().unwrap().id);
    assert_eq!(
        GitCli::new(&clone_dir).rev_parse("origin/dev").unwrap(),
        dev_sha2
    );

    std::fs::write(clone_dir.join("stale.tmp"), "leftover\n").unwrap();
    wait_done(&hub, &hub.reclone(&CloneRequest { url: None }).unwrap().id);
    assert!(!clone_dir.join("stale.tmp").exists());
    assert_eq!(hub.repo_status().head.as_deref(), Some(dev_sha2.as_str()));
}
