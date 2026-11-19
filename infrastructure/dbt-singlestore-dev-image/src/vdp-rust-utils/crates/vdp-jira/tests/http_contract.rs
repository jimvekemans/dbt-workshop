//! The real `JiraClient` against a local HTTP server: how HTTP statuses, dropped connections and bad bodies are
//! classified, end to end through `advance_tickets`.

use std::collections::HashSet;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tiny_http::{Header, Response, Server};
use vdp_core::BatchOutcome;
use vdp_jira::{IssueTracker, JiraClient, JiraError, TicketAdvance, advance_tickets};
use vdp_secrets::SecretString;

/// One request as the server saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    method: String,
    url: String,
    authorization: Option<String>,
    body: String,
}

struct Reply {
    status: u16,
    body: String,
}

fn reply(status: u16, body: &str) -> Reply {
    Reply {
        status,
        body: body.to_owned(),
    }
}

type Handler = Box<dyn FnMut(&Seen) -> Reply + Send>;

struct FakeJira {
    url: String,
    log: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl FakeJira {
    fn start(mut handler: Handler) -> Self {
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log_bg, stop_bg) = (Arc::clone(&log), Arc::clone(&stop));
        let handle = thread::spawn(move || {
            while !stop_bg.load(Ordering::Acquire) {
                let Ok(Some(mut request)) = server.recv_timeout(Duration::from_millis(15)) else {
                    continue;
                };
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let seen = Seen {
                    method: request.method().as_str().to_owned(),
                    url: request.url().to_owned(),
                    authorization: request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("Authorization"))
                        .map(|h| h.value.as_str().to_owned()),
                    body,
                };
                log_bg.lock().unwrap().push(seen.clone());
                let Reply { status, body } = handler(&seen);
                let json = Header::from_bytes("Content-Type", "application/json").unwrap();
                request
                    .respond(
                        Response::from_string(body)
                            .with_status_code(status)
                            .with_header(json),
                    )
                    .unwrap();
            }
        });
        Self {
            url,
            log,
            stop,
            handle: Some(handle),
        }
    }

    fn client(&self) -> JiraClient {
        client_for(&self.url)
    }

    fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }

    /// `"METHOD path"` without the query string, in arrival order.
    fn requests(&self) -> Vec<String> {
        self.seen()
            .iter()
            .map(|s| {
                let path = s.url.split('?').next().unwrap_or_default();
                format!("{} {path}", s.method)
            })
            .collect()
    }
}

impl Drop for FakeJira {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn client_for(base_url: &str) -> JiraClient {
    JiraClient::new(
        base_url,
        &SecretString::from("jane@example.com"),
        &SecretString::from("api-token"),
    )
}

fn always(status: u16, body: &'static str) -> Handler {
    Box::new(move |_| reply(status, body))
}

const OPEN_ISSUE: &str = r#"{"key":"INTEL-1","fields":{"summary":"S","status":{"name":"Open"}}}"#;

#[test]
fn a_401_is_a_fatal_auth_error() {
    let jira = FakeJira::start(always(401, r#"{"errorMessages":["nope"]}"#));
    let err = jira.client().ticket("INTEL-1").unwrap_err();
    assert!(
        matches!(err, JiraError::Auth { status: 401, .. }),
        "{err:?}"
    );
    assert!(err.is_fatal());
}

#[test]
fn a_403_is_a_fatal_auth_error() {
    let jira = FakeJira::start(always(403, "{}"));
    let err = jira.client().ticket("INTEL-1").unwrap_err();
    assert!(
        matches!(err, JiraError::Auth { status: 403, .. }),
        "{err:?}"
    );
    assert!(err.is_fatal());
}

#[test]
fn a_401_on_a_transition_post_is_also_auth() {
    let jira = FakeJira::start(always(401, "{}"));
    let err = jira.client().transition("INTEL-1", "21").unwrap_err();
    assert!(
        matches!(err, JiraError::Auth { status: 401, .. }),
        "{err:?}"
    );
}

#[test]
fn a_404_on_an_issue_is_a_per_issue_request_error() {
    let jira = FakeJira::start(always(404, r#"{"errorMessages":["Issue does not exist"]}"#));
    let err = jira.client().ticket("INTEL-404").unwrap_err();
    let JiraError::Request { url, detail } = &err else {
        panic!("expected Request, got {err:?}");
    };
    assert!(url.contains("/rest/api/2/issue/INTEL-404"), "{url}");
    assert!(detail.contains("404"), "{detail}");
    assert!(!err.is_fatal());
}

#[test]
fn a_500_is_a_per_issue_request_error() {
    let jira = FakeJira::start(always(500, "boom"));
    let err = jira.client().ticket("INTEL-1").unwrap_err();
    let JiraError::Request { detail, .. } = &err else {
        panic!("expected Request, got {err:?}");
    };
    assert!(detail.contains("500"), "{detail}");
    assert!(!err.is_fatal());
}

#[test]
fn a_refused_connection_is_a_fatal_transport_error() {
    // Bind to get a free port, then drop the listener so nothing is listening there any more.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = client_for(&format!("http://127.0.0.1:{port}"))
        .ticket("INTEL-1")
        .unwrap_err();
    assert!(matches!(err, JiraError::Transport { .. }), "{err:?}");
    assert!(err.is_fatal());
}

#[test]
fn a_200_with_invalid_json_is_a_response_error() {
    let jira = FakeJira::start(always(200, "<html>login page</html>"));
    let err = jira.client().ticket("INTEL-1").unwrap_err();
    let JiraError::Response { url, .. } = &err else {
        panic!("expected Response, got {err:?}");
    };
    assert!(url.contains("/rest/api/2/issue/INTEL-1"), "{url}");
    assert!(!err.is_fatal());
}

#[test]
fn a_200_without_fields_is_a_response_error() {
    let jira = FakeJira::start(always(200, r#"{"key":"INTEL-1"}"#));
    let err = jira.client().ticket("INTEL-1").unwrap_err();
    assert!(
        matches!(&err, JiraError::Response { detail, .. } if detail == "no fields"),
        "{err:?}"
    );
}

#[test]
fn requests_carry_basic_auth_and_the_sprint_field() {
    let jira = FakeJira::start(always(200, OPEN_ISSUE));
    let client = jira.client().with_sprint_field("customfield_99999");
    client.ticket("INTEL-1").unwrap();
    let seen = jira.seen();
    assert_eq!(seen.len(), 1);
    // base64("jane@example.com:api-token")
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some("Basic amFuZUBleGFtcGxlLmNvbTphcGktdG9rZW4=")
    );
    assert!(seen[0].url.starts_with("/rest/api/2/issue/INTEL-1?fields="));
    assert!(
        seen[0].url.ends_with(",customfield_99999"),
        "{}",
        seen[0].url
    );
}

#[test]
fn advance_isolates_a_404_advances_the_next_ticket_and_stops_at_the_401() {
    let done = Arc::new(Mutex::new(HashSet::<String>::new()));
    let jira = FakeJira::start(Box::new(move |seen| {
        let path = seen.url.split('?').next().unwrap_or_default().to_owned();
        match (seen.method.as_str(), path.as_str()) {
            (_, p) if p.contains("INTEL-1") => reply(404, r#"{"errorMessages":["gone"]}"#),
            (_, p) if p.contains("INTEL-3") => reply(401, "{}"),
            ("GET", "/rest/api/2/issue/INTEL-2") => {
                let status = if done.lock().unwrap().contains("INTEL-2") {
                    "Done"
                } else {
                    "Open"
                };
                reply(
                    200,
                    &format!(
                        r#"{{"key":"INTEL-2","fields":{{"summary":"S","status":{{"name":"{status}"}}}}}}"#
                    ),
                )
            }
            ("GET", "/rest/api/2/issue/INTEL-2/transitions") => reply(
                200,
                r#"{"transitions":[{"id":"21","name":"Resolve","to":{"name":"Done"}}]}"#,
            ),
            ("POST", "/rest/api/2/issue/INTEL-2/transitions") => {
                done.lock().unwrap().insert("INTEL-2".into());
                reply(204, "")
            }
            other => reply(500, &format!("unexpected {other:?}")),
        }
    }));
    let tickets: Vec<String> = ["INTEL-1", "INTEL-2", "INTEL-3", "INTEL-4"]
        .map(String::from)
        .into();

    let err = advance_tickets(&jira.client(), &tickets, "resolve").unwrap_err();

    assert!(
        matches!(err, JiraError::Auth { status: 401, .. }),
        "{err:?}"
    );
    assert_eq!(
        jira.requests(),
        [
            "GET /rest/api/2/issue/INTEL-1",
            "GET /rest/api/2/issue/INTEL-2",
            "GET /rest/api/2/issue/INTEL-2/transitions",
            "POST /rest/api/2/issue/INTEL-2/transitions",
            "GET /rest/api/2/issue/INTEL-2",
            "GET /rest/api/2/issue/INTEL-3",
        ],
        "ticket 4 must never be requested"
    );
    let post = jira
        .seen()
        .into_iter()
        .find(|s| s.method == "POST")
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&post.body).unwrap();
    assert_eq!(body, serde_json::json!({"transition": {"id": "21"}}));
}

#[test]
fn advance_reports_ticket_1_failed_and_ticket_2_advanced_before_the_fatal_error() {
    // The same scenario, observed through the per-ticket callback: the records before the fatal error are not lost.
    let done = Arc::new(Mutex::new(false));
    let jira = FakeJira::start(Box::new(move |seen| {
        let path = seen.url.split('?').next().unwrap_or_default().to_owned();
        if path.contains("INTEL-1") {
            return reply(404, "{}");
        }
        if path.contains("INTEL-3") {
            return reply(401, "{}");
        }
        match (seen.method.as_str(), path.ends_with("/transitions")) {
            ("GET", false) => {
                let status = if *done.lock().unwrap() {
                    "Done"
                } else {
                    "Open"
                };
                reply(
                    200,
                    &format!(r#"{{"fields":{{"status":{{"name":"{status}"}}}}}}"#),
                )
            }
            ("GET", true) => reply(
                200,
                r#"{"transitions":[{"id":"21","name":"Resolve","to":{"name":"Done"}}]}"#,
            ),
            _ => {
                *done.lock().unwrap() = true;
                reply(204, "")
            }
        }
    }));
    let tickets: Vec<String> = ["INTEL-1", "INTEL-2", "INTEL-3", "INTEL-4"]
        .map(String::from)
        .into();
    let mut records = Vec::new();
    let result = vdp_jira::advance_tickets_with(
        &jira.client(),
        &vdp_jira::AdvanceRequest {
            tickets: &tickets,
            wanted_transition: "resolve",
            dry_run: false,
        },
        |record| records.push(record.clone()),
    );

    assert!(matches!(result, Err(JiraError::Auth { status: 401, .. })));
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].key, "INTEL-1");
    let BatchOutcome::Failed(reason) = &records[0].outcome else {
        panic!("ticket 1 should have failed: {:?}", records[0]);
    };
    assert!(reason.contains("404"), "{reason}");
    assert_eq!(records[1].key, "INTEL-2");
    assert_eq!(
        records[1].outcome,
        BatchOutcome::Succeeded(TicketAdvance::Advanced {
            before: "Open".into(),
            after: "Done".into(),
            transition: "Resolve".into(),
        })
    );
}
