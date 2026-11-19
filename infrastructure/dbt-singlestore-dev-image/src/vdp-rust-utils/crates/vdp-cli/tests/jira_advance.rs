//! `vdp jira advance` as a process against a local fake Jira: what goes to stdout and stderr, in which order, and
//! with which exit code.
#![cfg(feature = "release")]

use std::collections::HashSet;
use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tiny_http::{Response, Server};

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

/// What the fake Jira does with a ticket.
#[derive(Clone, Copy)]
enum Ticket {
    /// Open, advances to Done with "Resolve".
    Ok,
    /// 404 on the first read.
    Missing,
    /// 401 on the first read.
    Unauthorized,
}

/// Lines the test has seen the child print so far, shared between the reader threads and the server.
#[derive(Default)]
struct Printed {
    stdout: Mutex<Vec<String>>,
    stderr: Mutex<Vec<String>>,
}

impl Printed {
    fn contains(&self, ticket: &str) -> bool {
        let has =
            |lines: &Mutex<Vec<String>>| lines.lock().unwrap().iter().any(|l| l.contains(ticket));
        has(&self.stdout) || has(&self.stderr)
    }
}

struct FakeJira {
    url: String,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    /// First-time reads of tickets, in order.
    first_reads: Arc<Mutex<Vec<String>>>,
    /// Tickets whose first read arrived before the child had printed the outcome of the ticket before it.
    printed_late: Arc<Mutex<Vec<String>>>,
}

impl FakeJira {
    /// `plan`: the tickets in the order the CLI is given them. A first read of ticket N waits (up to a few
    /// seconds) until the child has printed something about ticket N-1, so a CLI that holds its output back
    /// until the end shows up in `printed_late`.
    fn start(plan: &[(&str, Ticket)], printed: &Arc<Printed>) -> Self {
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let first_reads = Arc::new(Mutex::new(Vec::new()));
        let printed_late = Arc::new(Mutex::new(Vec::new()));
        let plan: Vec<(String, Ticket)> = plan.iter().map(|(k, t)| ((*k).to_owned(), *t)).collect();
        let (stop_bg, reads_bg, late_bg, printed) = (
            Arc::clone(&stop),
            Arc::clone(&first_reads),
            Arc::clone(&printed_late),
            Arc::clone(printed),
        );
        let handle = thread::spawn(move || {
            let mut done = HashSet::new();
            let mut started = HashSet::new();
            while !stop_bg.load(Ordering::Acquire) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(15)) else {
                    continue;
                };
                let path = request.url().split('?').next().unwrap().to_owned();
                let is_post = request.method().as_str() == "POST";
                let rest = path.strip_prefix("/rest/api/2/issue/").unwrap();
                let (key, tail) = rest.split_once('/').unwrap_or((rest, ""));
                let position = plan.iter().position(|(k, _)| k == key).unwrap();
                if started.insert(key.to_owned()) {
                    reads_bg.lock().unwrap().push(key.to_owned());
                    if position > 0 {
                        let previous = &plan[position - 1].0;
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while !printed.contains(previous) && Instant::now() < deadline {
                            thread::sleep(Duration::from_millis(5));
                        }
                        if !printed.contains(previous) {
                            late_bg.lock().unwrap().push(key.to_owned());
                        }
                    }
                }
                let (status, body) = match (plan[position].1, tail, is_post) {
                    (Ticket::Missing, ..) => (404, r#"{"errorMessages":["gone"]}"#.to_owned()),
                    (Ticket::Unauthorized, ..) => (401, "{}".to_owned()),
                    (Ticket::Ok, "transitions", false) => (
                        200,
                        r#"{"transitions":[{"id":"21","name":"Resolve","to":{"name":"Done"}}]}"#
                            .to_owned(),
                    ),
                    (Ticket::Ok, "transitions", true) => {
                        done.insert(key.to_owned());
                        (204, String::new())
                    }
                    (Ticket::Ok, _, _) => {
                        let status = if done.contains(key) { "Done" } else { "Open" };
                        (
                            200,
                            format!(
                                r#"{{"key":"{key}","fields":{{"summary":"S","status":{{"name":"{status}"}}}}}}"#
                            ),
                        )
                    }
                };
                request
                    .respond(Response::from_string(body).with_status_code(status))
                    .unwrap();
            }
        });
        Self {
            url,
            stop,
            handle: Some(handle),
            first_reads,
            printed_late,
        }
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

struct Run {
    code: Option<i32>,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

fn run_advance(dir: &Path, plan: &[(&str, Ticket)]) -> (Run, FakeJira) {
    let printed = Arc::new(Printed::default());
    let jira = FakeJira::start(plan, &printed);
    let config = dir.join("vdp.toml");
    std::fs::write(&config, format!("[jira]\nbase_url = \"{}\"\n", jira.url)).unwrap();

    let mut command = Command::new(VDP);
    command
        .args([
            "--no-vault",
            "jira",
            "advance",
            "--transition",
            "Resolve",
            "--tickets",
        ])
        .args(plan.iter().map(|(key, _)| key))
        .current_dir(dir)
        .env("VDP_CONFIG", &config)
        .env("JIRA_USER", "jane@example.com")
        .env("JIRA_TOKEN", "api-token")
        .env_remove("VDP_LOG")
        .env_remove("VDP_LOG_JSON")
        .env_remove("VDP_PROJECT_DIR")
        .env_remove("DBT_DEPLOY_TARGET");
    for proxy in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env_remove(proxy);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pump = |stream: Box<dyn std::io::Read + Send>,
                sink: fn(&Printed) -> &Mutex<Vec<String>>| {
        let printed = Arc::clone(&printed);
        thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                sink(&printed).lock().unwrap().push(line.unwrap());
            }
        })
    };
    let out = pump(Box::new(child.stdout.take().unwrap()), |p| &p.stdout);
    let err = pump(Box::new(child.stderr.take().unwrap()), |p| &p.stderr);
    let status = child.wait().unwrap();
    out.join().unwrap();
    err.join().unwrap();
    let run = Run {
        code: status.code(),
        stdout: printed.stdout.lock().unwrap().clone(),
        stderr: printed.stderr.lock().unwrap().clone(),
    };
    (run, jira)
}

#[test]
fn failures_go_to_stderr_advances_to_stdout_each_in_ticket_order_with_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let (run, jira) = run_advance(
        dir.path(),
        &[
            ("INTEL-1", Ticket::Ok),
            ("INTEL-2", Ticket::Missing),
            ("INTEL-3", Ticket::Ok),
            ("INTEL-4", Ticket::Missing),
            ("INTEL-5", Ticket::Ok),
        ],
    );

    assert_eq!(run.code, Some(1), "stderr: {:?}", run.stderr);
    assert_eq!(
        run.stdout,
        [
            "INTEL-1: 'Open' -> 'Done' (Resolve)",
            "INTEL-3: 'Open' -> 'Done' (Resolve)",
            "INTEL-5: 'Open' -> 'Done' (Resolve)",
        ]
    );
    let fields = "fields=summary,status,fixVersions,assignee,reporter,components,customfield_11221";
    assert_eq!(
        run.stderr,
        [
            format!(
                " WARN INTEL-2: Jira request {0}/rest/api/2/issue/INTEL-2?{fields} failed: http status: 404",
                jira.url
            ),
            format!(
                " WARN INTEL-4: Jira request {0}/rest/api/2/issue/INTEL-4?{fields} failed: http status: 404",
                jira.url
            ),
        ]
    );
}

#[test]
fn each_ticket_line_is_printed_before_the_next_ticket_is_requested() {
    let dir = tempfile::tempdir().unwrap();
    let (run, jira) = run_advance(
        dir.path(),
        &[
            ("INTEL-1", Ticket::Ok),
            ("INTEL-2", Ticket::Missing),
            ("INTEL-3", Ticket::Ok),
            ("INTEL-4", Ticket::Missing),
        ],
    );

    assert_eq!(run.code, Some(1), "stderr: {:?}", run.stderr);
    assert_eq!(
        *jira.first_reads.lock().unwrap(),
        ["INTEL-1", "INTEL-2", "INTEL-3", "INTEL-4"]
    );
    // The fake server holds back the first read of ticket N until the line about ticket N-1 reached the test
    // (on stdout for an advance, on stderr for a failure). Output that waits for the end of the run misses it.
    assert_eq!(
        *jira.printed_late.lock().unwrap(),
        Vec::<String>::new(),
        "the outcome of the previous ticket was not printed before the next ticket was requested"
    );
}

#[test]
fn an_auth_failure_stops_the_batch_after_the_tickets_before_it_were_reported() {
    let dir = tempfile::tempdir().unwrap();
    let (run, jira) = run_advance(
        dir.path(),
        &[
            ("INTEL-1", Ticket::Ok),
            ("INTEL-2", Ticket::Unauthorized),
            ("INTEL-3", Ticket::Ok),
        ],
    );

    assert_eq!(run.stdout, ["INTEL-1: 'Open' -> 'Done' (Resolve)"]);
    assert_eq!(*jira.first_reads.lock().unwrap(), ["INTEL-1", "INTEL-2"]);
    // A fatal error is a toolkit error (exit 2), not a batch failure (exit 1).
    assert_eq!(run.code, Some(2), "stderr: {:?}", run.stderr);
    let stderr = run.stderr.join("\n");
    assert!(stderr.contains("authentication failed"), "{stderr}");
    assert!(stderr.contains("401"), "{stderr}");
    assert!(!stderr.contains("INTEL-3"), "{stderr}");
}
