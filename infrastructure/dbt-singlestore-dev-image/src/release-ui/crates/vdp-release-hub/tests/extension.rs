//! The plug-in boundary without any real extension: a fake one is routed to, keyed in the status, and its
//! rights and job results travel in the hub's own documents under its segment.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};
use vdp_notify::{Announcement, Delivery, Mailer, NotifyError};
use vdp_release::ReleaseSettings;
use vdp_release_hub::auth::{BranchRight, SessionUser};
use vdp_release_hub::http::{HttpRequest, Reply, route};
use vdp_release_hub::jobs::{JobKind, JobResult};
use vdp_release_hub::{ExtensionRequest, GitLab, Hub, HubConfig, HubExtension, HubServices};

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

/// Answers every request with what it was asked, so routing can be read off the reply.
#[derive(Debug)]
struct Fake;

impl HubExtension for Fake {
    #[allow(clippy::unnecessary_literal_bound, reason = "the trait's lifetime")]
    fn segment(&self) -> &str {
        "fake"
    }

    fn summary(&self) -> Value {
        json!({ "repo_url": "git@example.com:fake.git" })
    }

    fn rights(&self, _: &GitLab, _: &str) -> Vec<BranchRight> {
        Vec::new()
    }

    fn handle(&self, request: &ExtensionRequest<'_>) -> Reply {
        Reply::json(
            202,
            &json!({
                "method": request.method,
                "segments": request.segments,
                "param": request.param("x"),
                "body": request.json::<Value>().ok(),
            }),
        )
    }
}

fn hub(extensions: Vec<Arc<dyn HubExtension>>) -> Arc<Hub> {
    let dir = tempfile::tempdir().unwrap();
    Hub::new(
        HubConfig {
            repo_dir: dir.path().join("clone"),
            repo_url: "git@example.com:dbt.git".into(),
            release: ReleaseSettings::default(),
            jira_base_url: String::new(),
            excluded_fix_versions: Vec::new(),
            default_recipients: Vec::new(),
            keep_jobs: 5,
            git_timeout: None,
            conflict_timeout: std::time::Duration::from_secs(1),
            state_dir: None,
            gitlab: None,
        },
        HubServices {
            tracker: Err("no Jira in tests".into()),
            mailer: Arc::new(SilentMailer),
            validator: None,
            extensions,
        },
    )
}

fn json_of(reply: &Reply) -> Value {
    serde_json::from_slice(&reply.body).unwrap()
}

#[test]
fn requests_below_the_segment_reach_the_extension() {
    let hub = hub(vec![Arc::new(Fake)]);
    let reply = route(
        &hub,
        HttpRequest {
            method: "POST",
            path: "/fake/jobs/j1/resolution/",
            query: "x=a%20b",
            body: br#"{"k":1}"#,
            session: None,
        },
    );
    assert_eq!(reply.status, 202);
    assert_eq!(
        json_of(&reply),
        json!({
            "method": "POST",
            "segments": ["jobs", "j1", "resolution"],
            "param": "a b",
            "body": { "k": 1 },
        })
    );
}

#[test]
fn an_unregistered_segment_is_not_found() {
    let reply = route(
        &hub(Vec::new()),
        HttpRequest {
            method: "GET",
            path: "/fake/status",
            query: "",
            body: b"",
            session: None,
        },
    );
    assert_eq!(reply.status, 404);
    // The dbt side is untouched by extensions.
    let status = route(
        &hub(vec![Arc::new(Fake)]),
        HttpRequest {
            method: "GET",
            path: "/status",
            query: "",
            body: b"",
            session: None,
        },
    );
    assert_eq!(status.status, 200);
}

#[test]
fn the_status_keys_each_summary_by_segment() {
    let with = json_of(&route(
        &hub(vec![Arc::new(Fake)]),
        HttpRequest {
            method: "GET",
            path: "/status",
            query: "",
            body: b"",
            session: None,
        },
    ));
    assert_eq!(
        with["fake"],
        json!({ "repo_url": "git@example.com:fake.git" })
    );
    assert_eq!(with["auth"], "none");
    let without = json_of(&route(
        &hub(Vec::new()),
        HttpRequest {
            method: "GET",
            path: "/dbt/status",
            query: "",
            body: b"",
            session: None,
        },
    ));
    assert!(without.get("fake").is_none());
}

#[test]
fn rights_and_job_results_use_the_extensions_own_names() {
    let mut user = SessionUser {
        username: "u".into(),
        name: "U".into(),
        email: "u@example.com".into(),
        can_release: true,
        reasons: Vec::new(),
        rights: BTreeMap::default(),
    };
    user.rights.insert(
        "fake".into(),
        vec![BranchRight {
            branch: "feature/*".into(),
            push: true,
            force: false,
            reason: None,
        }],
    );
    let value = serde_json::to_value(&user).unwrap();
    assert_eq!(value["fake"][0]["branch"], "feature/*");
    assert!(value.get("rights").is_none());
    assert_eq!(serde_json::from_value::<SessionUser>(value).unwrap(), user);
    assert!(
        user.right("fake", "feature/2026_DROP1")
            .is_some_and(|r| r.push)
    );
    assert!(user.right("other", "feature/x").is_none());

    let kind = JobKind::Other("fake_build".into());
    assert_eq!(serde_json::to_value(&kind).unwrap(), "fake_build");
    assert_eq!(
        serde_json::from_value::<JobKind>(json!("sync")).unwrap(),
        JobKind::Sync
    );
    assert_eq!(
        serde_json::from_value::<JobKind>(json!("fake_build")).unwrap(),
        kind
    );

    let result = JobResult::Extension(json!({ "type": "fake_done", "n": 1 }));
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(value, json!({ "type": "fake_done", "n": 1 }));
    assert_eq!(serde_json::from_value::<JobResult>(value).unwrap(), result);
    let known = json!({ "type": "analysis", "analysis_id": "a1" });
    assert!(matches!(
        serde_json::from_value::<JobResult>(known).unwrap(),
        JobResult::Analysis { .. }
    ));
}
