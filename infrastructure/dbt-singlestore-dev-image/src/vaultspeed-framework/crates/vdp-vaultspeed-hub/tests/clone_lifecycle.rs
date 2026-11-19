//! End-to-end clone lifecycle tests for `VsHub` against a local bare repository (no network).

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use vdp_git::Vcs;
use vdp_jira::{IssueTracker, JiraError, Ticket, Transition};
use vdp_release_hub::HubError;
use vdp_release_hub::api::CloneRequest;
use vdp_release_hub::jobs::{Job, JobStatus};
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed_hub::{VsConfig, VsHub};
use vdp_vaultspeed_release::VsReleaseSettings;

#[derive(Debug)]
struct CountingTracker(Arc<AtomicUsize>);

impl IssueTracker for CountingTracker {
    fn ticket(&self, key: &str) -> Result<Ticket, JiraError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Ticket {
            key: key.to_owned(),
            summary: format!("Summary {key}"),
            status: "Open".into(),
            ..Ticket::default()
        })
    }

    fn transitions(&self, _: &str) -> Result<Vec<Transition>, JiraError> {
        Ok(Vec::new())
    }

    fn transition(&self, _: &str, _: &str) -> Result<(), JiraError> {
        Ok(())
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

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Writes a valid stored (uncompressed) single-entry zip without external deps.
fn stored_zip(entry: &str, content: &[u8]) -> Vec<u8> {
    let crc = crc32(content);
    let len = u32::try_from(content.len()).unwrap();
    let name_len = u16::try_from(entry.len()).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(&0x0403_4B50_u32.to_le_bytes());
    out.extend_from_slice(&20_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&name_len.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(entry.as_bytes());
    out.extend_from_slice(content);
    let cd_offset = u32::try_from(out.len()).unwrap();
    out.extend_from_slice(&0x0201_4B50_u32.to_le_bytes());
    out.extend_from_slice(&20_u16.to_le_bytes());
    out.extend_from_slice(&20_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&name_len.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(entry.as_bytes());
    let cd_size = u32::try_from(out.len()).unwrap() - cd_offset;
    out.extend_from_slice(&0x0605_4B50_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out
}

fn wait_done(hub: &VsHub, id: &str) -> Job {
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

fn seed_bare_remote(root: &Path) -> (std::path::PathBuf, std::path::PathBuf, String, String) {
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
    git_run(&work, &["checkout", "-b", "master"]);
    std::fs::write(work.join("README.md"), "master1\n").unwrap();
    git_run(&work, &["add", "README.md"]);
    git_run(&work, &["commit", "-m", "master commit 1"]);
    git_run(&work, &["push", "-u", "origin", "master"]);
    let master_sha1 = git_run(&work, &["rev-parse", "HEAD"]);

    git_run(&work, &["checkout", "-b", "main"]);
    std::fs::write(work.join("README.md"), "main-decoy\n").unwrap();
    git_run(&work, &["commit", "-am", "main decoy commit"]);
    git_run(&work, &["push", "-u", "origin", "main"]);
    let main_decoy_sha = git_run(&work, &["rev-parse", "HEAD"]);
    assert_ne!(master_sha1, main_decoy_sha);

    git_run(&work, &["checkout", "-b", "dev", "master"]);
    std::fs::write(work.join("README.md"), "dev1\n").unwrap();
    git_run(&work, &["commit", "-am", "dev commit"]);
    git_run(&work, &["push", "-u", "origin", "dev"]);
    let dev_sha = git_run(&work, &["rev-parse", "HEAD"]);

    git_run(&work, &["checkout", "-b", "release/1182_FMC", "master"]);
    let staging = work.join("vaultspeed/staging/release");
    std::fs::create_dir_all(&staging).unwrap();
    let zip_bytes = stored_zip(
        "1182_FMC_x.sql",
        b"-- Release: Release_1(1) - Comment: INTEL-100 - Release date: 2026/01/01\nSELECT 1;\n",
    );
    std::fs::write(staging.join("1182_FMC.zip"), zip_bytes).unwrap();
    git_run(&work, &["add", "-A"]);
    git_run(&work, &["commit", "-m", "stage 1182_FMC"]);
    git_run(&work, &["push", "-u", "origin", "release/1182_FMC"]);
    git_run(&work, &["checkout", "master"]);
    (origin, work, master_sha1, dev_sha)
}

#[test]
fn vs_hub_clone_lifecycle_exercises_main_detach_branch_health_sync_ticket_clear_and_reclone() {
    let dir = tempfile::tempdir().unwrap();
    let (origin, work, master_sha1, dev_sha) = seed_bare_remote(dir.path());
    assert_ne!(master_sha1, dev_sha);
    let clone_dir = dir.path().join("vs_clone");
    let calls = Arc::new(AtomicUsize::new(0));
    let tracker: Arc<dyn IssueTracker> = Arc::new(CountingTracker(Arc::clone(&calls)));
    let hub = VsHub::new(
        VsConfig {
            repo_dir: clone_dir.clone(),
            repo_url: origin.to_string_lossy().into_owned(),
            gitlab: None,
            release: VsReleaseSettings {
                main: "master".into(),
                ..VsReleaseSettings::default()
            },
            vaultspeed: VaultspeedSettings::default(),
            keep_jobs: 10,
            git_timeout: None,
            conflict_timeout: Duration::from_secs(1),
            state_dir: None,
        },
        Some(tracker),
    );

    assert!(!hub.repo_status().cloned);
    for err in [hub.sync().unwrap_err(), hub.repair().unwrap_err()] {
        assert!(
            matches!(err, HubError::Conflict(ref m) if m == "the VaultSpeed repository is not cloned yet"),
            "{err:?}"
        );
    }

    wait_done(
        &hub,
        &hub.clone_repo(&CloneRequest { url: None }).unwrap().id,
    );
    let st = hub.repo_status();
    assert!(st.cloned);
    assert_eq!(st.head.as_deref(), Some(master_sha1.as_str()));
    assert_eq!(st.health, vec![]);

    let again = hub.clone_repo(&CloneRequest { url: None }).unwrap_err();
    assert!(
        matches!(again, HubError::Conflict(ref m) if m == "the VaultSpeed repository is already cloned: sync it instead")
    );

    git_run(&clone_dir, &["checkout", "-b", "dev"]);
    let unhealthy = hub.repo_status();
    assert!(
        unhealthy
            .health
            .iter()
            .any(|p| p.code == "local_branches" && p.detail == "local branches left over: dev"),
        "unexpected health: {:?}",
        unhealthy.health
    );

    wait_done(&hub, &hub.repair().unwrap().id);
    let repaired = hub.repo_status();
    assert_eq!(repaired.health, vec![]);
    assert_eq!(repaired.head.as_deref(), Some(master_sha1.as_str()));

    let _ = hub.status(false).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    std::fs::write(work.join("README.md"), "master2\n").unwrap();
    git_run(&work, &["commit", "-am", "master commit 2"]);
    git_run(&work, &["push", "origin", "master"]);
    let master_sha2 = git_run(&work, &["rev-parse", "HEAD"]);

    wait_done(&hub, &hub.sync().unwrap().id);
    assert_eq!(
        vdp_git::GitCli::new(&clone_dir)
            .rev_parse("origin/master")
            .unwrap(),
        master_sha2
    );
    let _ = hub.status(false).unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "sync must clear TicketCache"
    );

    std::fs::write(clone_dir.join("stale.tmp"), "leftover\n").unwrap();
    wait_done(&hub, &hub.reclone(&CloneRequest { url: None }).unwrap().id);
    assert!(!clone_dir.join("stale.tmp").exists());
    assert_eq!(
        hub.repo_status().head.as_deref(),
        Some(master_sha2.as_str())
    );
}
