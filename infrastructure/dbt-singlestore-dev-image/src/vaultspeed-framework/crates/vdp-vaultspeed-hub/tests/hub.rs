//! The VaultSpeed extension inside a real hub: routes, summary keying and the documents that carry its jobs.
//! No GitLab and no clone: what needs a repository answers "not cloned" (409), which is itself the contract.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use vdp_notify::{Announcement, Delivery, Mailer, NotifyError};
use vdp_release::ReleaseSettings;
use vdp_release_hub::http::{HttpRequest, Reply, route};
use vdp_release_hub::jobs::JobResult;
use vdp_release_hub::{Hub, HubConfig, HubExtension, HubServices};
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed_hub::api::VsJobResult;
use vdp_vaultspeed_hub::{VaultspeedExtension, VsConfig};
use vdp_vaultspeed_release::VsReleaseSettings;

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

fn hub(dir: &std::path::Path) -> Arc<Hub> {
    let extension: Arc<dyn HubExtension> = Arc::new(VaultspeedExtension::new(
        VsConfig {
            repo_dir: dir.join("vaultspeed"),
            repo_url: "git@example.com:vdp/vaultspeed-automation-framework.git".into(),
            gitlab: None,
            release: VsReleaseSettings::default(),
            vaultspeed: VaultspeedSettings::default(),
            keep_jobs: 5,
            git_timeout: None,
            conflict_timeout: Duration::from_secs(1),
            state_dir: None,
        },
        None,
    ));
    Hub::new(
        HubConfig {
            repo_dir: dir.join("dbt"),
            repo_url: "git@example.com:vdp/dbt.git".into(),
            release: ReleaseSettings::default(),
            jira_base_url: String::new(),
            excluded_fix_versions: Vec::new(),
            default_recipients: Vec::new(),
            keep_jobs: 5,
            git_timeout: None,
            conflict_timeout: Duration::from_secs(1),
            state_dir: None,
            gitlab: None,
        },
        HubServices {
            tracker: Err("no Jira in tests".into()),
            mailer: Arc::new(SilentMailer),
            validator: None,
            extensions: vec![extension],
        },
    )
}

fn call(hub: &Arc<Hub>, method: &str, path: &str, query: &str, body: &str) -> (u16, Value) {
    let Reply { status, body, .. } = route(
        hub,
        HttpRequest {
            method,
            path,
            query,
            body: body.as_bytes(),
            session: None,
        },
    );
    (status, serde_json::from_slice(&body).unwrap())
}

#[test]
fn the_status_carries_the_summary_under_vaultspeed() {
    let dir = tempfile::tempdir().unwrap();
    let hub = hub(dir.path());
    let (status, body) = call(&hub, "GET", "/status", "", "");
    assert_eq!(status, 200);
    let summary = &body["vaultspeed"];
    let settings = VsReleaseSettings::default();
    assert_eq!(summary["main"], settings.main.as_str());
    assert_eq!(summary["release_prefix"], settings.release_prefix.as_str());
    assert_eq!(summary["repo"]["cloned"], false);
    assert!(summary["environments"].is_array());
    assert!(summary["active_job"].is_null());
}

#[test]
fn routes_keep_their_status_codes() {
    let dir = tempfile::tempdir().unwrap();
    let hub = hub(dir.path());
    // Not cloned yet: 409 for what reads the repository.
    for path in ["/vaultspeed/status", "/vaultspeed/tree"] {
        let query = if path.ends_with("tree") {
            "branch=main"
        } else {
            ""
        };
        assert_eq!(call(&hub, "GET", path, query, "").0, 409, "{path}");
    }
    // A branch the UI never shows: 400 before anything else.
    assert_eq!(
        call(&hub, "GET", "/vaultspeed/file", "branch=nope&path=a", "").0,
        400
    );
    assert_eq!(
        call(&hub, "GET", "/vaultspeed/changes", "from=x%20y&to=z", "").0,
        409
    );
    assert_eq!(call(&hub, "GET", "/vaultspeed/jobs", "", "").0, 200);
    assert_eq!(call(&hub, "GET", "/vaultspeed/jobs/j9", "", "").0, 404);
    assert_eq!(
        call(&hub, "GET", "/vaultspeed/jobs/j9/conflict", "", "").0,
        400
    );
    assert_eq!(
        call(&hub, "GET", "/vaultspeed/pipeline", "sha=abcdef1", "").1,
        Value::Null
    );
    // Unknown routes and bodies that do not fit.
    assert_eq!(call(&hub, "GET", "/vaultspeed/nothing/here", "", "").0, 404);
    assert_eq!(
        call(&hub, "POST", "/vaultspeed/merges", "", r#"{"nope":1}"#).0,
        400
    );
    // A job needs the clone, too.
    let release = json!({ "request": { "drop": "2026_DROP1" } }).to_string();
    let (status, _) = call(&hub, "POST", "/vaultspeed/releases", "", &release);
    assert!(matches!(status, 400 | 409), "{status}");
}

#[test]
fn job_results_keep_the_vs_type_names() {
    let commit =
        json!({ "hash": "abc", "short": "a", "subject": "s", "author": "me", "timestamp": 1 });
    let result = VsJobResult::Reset {
        outcome: serde_json::from_value(json!({
            "branch": "main",
            "origin_head": "def",
            "target": commit,
            "losses": [],
            "pushed": false,
        }))
        .unwrap(),
    };
    let JobResult::Extension(value) = result.into_result() else {
        panic!("an extension result");
    };
    assert_eq!(value["type"], "vs_reset");
    assert_eq!(value["outcome"]["origin_head"], "def");
    assert_eq!(
        serde_json::from_value::<JobResult>(value.clone()).unwrap(),
        JobResult::Extension(value)
    );
}
