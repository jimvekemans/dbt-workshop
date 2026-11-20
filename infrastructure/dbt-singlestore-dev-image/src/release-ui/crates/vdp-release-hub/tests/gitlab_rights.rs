//! GitLab rights over HTTP, against a local stand-in for GitLab: the protected branches are read page after page, so a rule
//! beyond the first hundred still counts.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{Value, json};
use tiny_http::{Header, Response, Server};
use vdp_release_hub::GitLab;
use vdp_release_hub::auth::GitLabUser;

/// A GitLab project with 100 protected branches that say nothing about the release branches, and on the second page the
/// rule that matters: `release/*` is protected without "Allowed to force push". Records every request it gets.
struct FakeGitLab {
    server: Arc<Server>,
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    worker: Option<JoinHandle<()>>,
}

impl FakeGitLab {
    fn start() -> Self {
        let server = Arc::new(Server::http("127.0.0.1:0").unwrap());
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let worker = {
            let (server, requests) = (Arc::clone(&server), Arc::clone(&requests));
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    let path = request.url().to_owned();
                    requests.lock().unwrap().push(path.clone());
                    let body = answer(&path);
                    let header = Header::from_bytes("Content-Type", "application/json").unwrap();
                    let _ = request
                        .respond(Response::from_string(body.to_string()).with_header(header));
                }
            })
        };
        Self {
            server,
            url,
            requests,
            worker: Some(worker),
        }
    }

    fn gitlab(&self) -> GitLab {
        GitLab {
            url: self.url.clone(),
            project: "vdp/dbt-vdp".into(),
            push_url: format!("{}/vdp/dbt-vdp.git", self.url),
        }
    }
}

impl Drop for FakeGitLab {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn answer(path: &str) -> Value {
    let protected = |name: &str, force: bool| {
        json!({
            "name": name,
            "push_access_levels": [{ "access_level": 40, "user_id": null, "group_id": null }],
            "allow_force_push": force,
        })
    };
    match path {
        "/api/v4/projects/vdp%2Fdbt-vdp" => {
            json!({ "id": 7, "permissions": { "project_access": { "access_level": 40 } } })
        }
        "/api/v4/projects/7/protected_branches?per_page=100&page=1" => Value::Array(
            (0..100)
                .map(|n| protected(&format!("team/{n}/*"), true))
                .collect(),
        ),
        "/api/v4/projects/7/protected_branches?per_page=100&page=2" => {
            json!([protected("release/*", false)])
        }
        other => panic!("unexpected request {other}"),
    }
}

#[test]
fn a_rule_on_the_second_page_of_protected_branches_counts() {
    let fake = FakeGitLab::start();
    let user = GitLabUser {
        id: 1,
        username: "person1".into(),
        name: "Person One".into(),
        email: "person1@example.com".into(),
    };
    let rights = fake
        .gitlab()
        .rights(
            "token",
            &user,
            &[
                ("main".into(), "main".into()),
                ("release/*".into(), "release/2099_DROP99".into()),
            ],
        )
        .unwrap();

    // `main` has no rule: a Maintainer may force-push it. `release/*` is the rule of page 2: no force push.
    assert!(rights[0].push && rights[0].force, "{:?}", rights[0]);
    assert!(rights[1].push && !rights[1].force, "{:?}", rights[1]);
    assert!(
        rights[1]
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("without \"Allowed to force push\"")),
        "{:?}",
        rights[1]
    );
    assert_eq!(
        fake.requests.lock().unwrap().as_slice(),
        [
            "/api/v4/projects/vdp%2Fdbt-vdp",
            "/api/v4/projects/7/protected_branches?per_page=100&page=1",
            "/api/v4/projects/7/protected_branches?per_page=100&page=2",
        ]
    );
}
