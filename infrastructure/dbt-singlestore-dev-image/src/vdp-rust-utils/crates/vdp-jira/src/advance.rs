use vdp_core::{BatchOutcome, BatchRecord, BatchReport};

use crate::{IssueTracker, JiraError};

/// How a ticket that did not fail was handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketAdvance {
    Advanced {
        before: String,
        after: String,
        transition: String,
    },
    /// Predicted outcome in `--dry-run` mode; no transition POST was sent.
    WouldAdvance {
        before: String,
        after: String,
        transition: String,
    },
    /// The transition POST succeeded, but the follow-up ticket read failed.
    MovedUnknownAfter {
        before: String,
        transition: String,
        warning: String,
    },
}

/// Per-ticket outcomes from [`advance_tickets`]: keyed by ticket, a failure carries its reason. A ticket that
/// failed does not stop the others; only authentication and transport errors do, as `Err`.
pub type AdvanceReport = BatchReport<String, TicketAdvance, String>;

/// One ticket of an [`AdvanceReport`].
pub type AdvanceRecord = BatchRecord<String, TicketAdvance, String>;

/// Inputs for [`advance_tickets_with`].
#[derive(Debug, Clone, Copy)]
pub struct AdvanceRequest<'a> {
    pub tickets: &'a [String],
    pub wanted_transition: &'a str,
    pub dry_run: bool,
}

/// Advances each ticket in `tickets` to `wanted_transition` (or the first offered transition when empty),
/// isolating per-ticket errors while stopping immediately on authentication or transport failures.
///
/// # Errors
///
/// [`JiraError`] only when it is fatal ([`JiraError::is_fatal`]: authentication or transport failures); other
/// per-ticket failures are recorded in the [`AdvanceReport`].
pub fn advance_tickets(
    tracker: &dyn IssueTracker,
    tickets: &[String],
    wanted_transition: &str,
) -> Result<AdvanceReport, JiraError> {
    advance_tickets_with(
        tracker,
        &AdvanceRequest {
            tickets,
            wanted_transition,
            dry_run: false,
        },
        |_| {},
    )
}

/// Advances tickets per `request`, calling `on_ticket` immediately as each ticket finishes before the next
/// ticket is requested.
///
/// # Errors
///
/// Same as [`advance_tickets`]: only fatal errors stop the batch; `on_ticket` has already been called for the tickets
/// before it.
pub fn advance_tickets_with(
    tracker: &dyn IssueTracker,
    request: &AdvanceRequest<'_>,
    mut on_ticket: impl FnMut(&AdvanceRecord),
) -> Result<AdvanceReport, JiraError> {
    let mut report = AdvanceReport::with_capacity(request.tickets.len());
    for key in request.tickets {
        let outcome = advance_one(tracker, key, request.wanted_transition, request.dry_run)?;
        let record = AdvanceRecord::new(key.clone(), outcome);
        on_ticket(&record);
        report.push_record(record);
    }
    Ok(report)
}

type TicketOutcome = BatchOutcome<TicketAdvance, String>;

fn classify_error(err: JiraError) -> Result<TicketOutcome, JiraError> {
    if err.is_fatal() {
        Err(err)
    } else {
        Ok(BatchOutcome::Failed(err.to_string()))
    }
}

fn advance_one(
    tracker: &dyn IssueTracker,
    key: &str,
    wanted: &str,
    dry_run: bool,
) -> Result<TicketOutcome, JiraError> {
    let before = match tracker.ticket(key) {
        Ok(ticket) => ticket,
        Err(err) => return classify_error(err),
    };
    let options = match tracker.transitions(key) {
        Ok(options) => options,
        Err(err) => return classify_error(err),
    };
    let chosen = if wanted.is_empty() {
        options.first()
    } else {
        options
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(wanted) || t.to.eq_ignore_ascii_case(wanted))
    };
    let Some(chosen) = chosen else {
        let names: Vec<&str> = options.iter().map(|t| t.name.as_str()).collect();
        return Ok(BatchOutcome::Failed(format!(
            "no transition {wanted:?} from '{}' (offered: {})",
            before.status,
            names.join(", ")
        )));
    };
    if dry_run {
        return Ok(BatchOutcome::Succeeded(TicketAdvance::WouldAdvance {
            before: before.status,
            after: chosen.to.clone(),
            transition: chosen.name.clone(),
        }));
    }
    if let Err(err) = tracker.transition(key, &chosen.id) {
        return classify_error(err);
    }
    match tracker.ticket(key) {
        Ok(after) => Ok(BatchOutcome::Succeeded(TicketAdvance::Advanced {
            before: before.status,
            after: after.status,
            transition: chosen.name.clone(),
        })),
        Err(err) => Ok(BatchOutcome::Succeeded(TicketAdvance::MovedUnknownAfter {
            before: before.status,
            transition: chosen.name.clone(),
            warning: err.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Mutex;

    use crate::{JiraError, Ticket, Transition};

    use super::*;

    fn succeeded(key: &str, advance: TicketAdvance) -> AdvanceRecord {
        AdvanceRecord::new(key.into(), BatchOutcome::Succeeded(advance))
    }

    fn advanced(key: &str, before: &str, after: &str, transition: &str) -> AdvanceRecord {
        succeeded(
            key,
            TicketAdvance::Advanced {
                before: before.into(),
                after: after.into(),
                transition: transition.into(),
            },
        )
    }

    fn would_advance(key: &str, before: &str, after: &str, transition: &str) -> AdvanceRecord {
        succeeded(
            key,
            TicketAdvance::WouldAdvance {
                before: before.into(),
                after: after.into(),
                transition: transition.into(),
            },
        )
    }

    #[derive(Debug, Default)]
    struct FakeTracker {
        fail_ticket_read: Option<String>,
        fail_after_transition_read: Option<String>,
        fail_transition: Option<String>,
        fatal_on_ticket: Option<(String, JiraError)>,
        requested: Mutex<Vec<String>>,
        events: Option<std::sync::Arc<Mutex<Vec<String>>>>,
        moved: Mutex<Vec<(String, String)>>,
        done: Mutex<HashSet<String>>,
    }

    impl FakeTracker {
        fn requested(&self) -> Vec<String> {
            self.requested.lock().unwrap().clone()
        }

        fn transitioned(&self) -> Vec<(String, String)> {
            self.moved.lock().unwrap().clone()
        }
    }

    impl IssueTracker for FakeTracker {
        fn ticket(&self, key: &str) -> Result<Ticket, JiraError> {
            self.requested.lock().unwrap().push(key.to_owned());
            if let Some(events) = &self.events {
                events.lock().unwrap().push(format!("request:{key}"));
            }
            if let Some((target, err)) = &self.fatal_on_ticket
                && target == key
            {
                return Err(err.clone());
            }
            if self.fail_ticket_read.as_deref() == Some(key) {
                return Err(JiraError::Request {
                    url: format!("https://jira.example/rest/api/2/issue/{key}"),
                    detail: "HTTP 500 Internal Server Error".into(),
                });
            }
            let is_done = self.done.lock().unwrap().contains(key);
            if is_done && self.fail_after_transition_read.as_deref() == Some(key) {
                return Err(JiraError::Request {
                    url: format!("https://jira.example/rest/api/2/issue/{key}"),
                    detail: "HTTP 500 on follow-up read".into(),
                });
            }
            let status = if is_done { "Done" } else { "Open" };
            Ok(Ticket {
                key: key.to_owned(),
                status: status.to_owned(),
                ..Ticket::default()
            })
        }

        fn transitions(&self, _key: &str) -> Result<Vec<Transition>, JiraError> {
            Ok(vec![
                Transition {
                    id: "11".into(),
                    name: "Start Progress".into(),
                    to: "In Progress".into(),
                },
                Transition {
                    id: "21".into(),
                    name: "Resolve".into(),
                    to: "Done".into(),
                },
            ])
        }

        fn transition(&self, key: &str, transition_id: &str) -> Result<(), JiraError> {
            if self.fail_transition.as_deref() == Some(key) {
                return Err(JiraError::Request {
                    url: format!("https://jira.example/rest/api/2/issue/{key}/transitions"),
                    detail: "HTTP 500 Internal Server Error".into(),
                });
            }
            self.moved
                .lock()
                .unwrap()
                .push((key.to_owned(), transition_id.to_owned()));
            self.done.lock().unwrap().insert(key.to_owned());
            Ok(())
        }
    }

    #[test]
    fn ticket_error_does_not_stop_other_tickets() {
        let tracker = FakeTracker {
            fail_transition: Some("INTEL-2".into()),
            ..FakeTracker::default()
        };
        let tickets = vec!["INTEL-1".into(), "INTEL-2".into(), "INTEL-3".into()];
        let report = advance_tickets(&tracker, &tickets, "done").unwrap();

        assert!(report.has_failures());
        assert_eq!(
            tracker.transitioned(),
            [
                ("INTEL-1".into(), "21".into()),
                ("INTEL-3".into(), "21".into()),
            ]
        );
        let records = report.records();
        assert_eq!(records[0], advanced("INTEL-1", "Open", "Done", "Resolve"));
        let BatchOutcome::Failed(reason) = &records[1].outcome else {
            panic!("expected failure for INTEL-2, got {:?}", records[1]);
        };
        assert_eq!(records[1].key, "INTEL-2");
        assert!(reason.contains("500"), "{reason}");
        assert_eq!(records[2], advanced("INTEL-3", "Open", "Done", "Resolve"));
    }

    #[test]
    fn missing_transition_records_offered_names_and_continues() {
        let tracker = FakeTracker::default();
        let tickets = vec!["INTEL-1".into(), "INTEL-2".into()];
        let report = advance_tickets(&tracker, &tickets, "Deploy").unwrap();
        assert!(report.has_failures());
        assert_eq!(tracker.transitioned(), []);
        let records = report.records();
        let BatchOutcome::Failed(reason) = &records[0].outcome else {
            panic!("expected failure, got {:?}", records[0]);
        };
        assert_eq!(records[0].key, "INTEL-1");
        assert!(reason.contains("Start Progress, Resolve"), "{reason}");
        assert!(matches!(
            &records[1],
            BatchRecord { key, outcome: BatchOutcome::Failed(_) } if key == "INTEL-2"
        ));
    }

    #[test]
    fn all_tickets_succeed_when_transition_matches() {
        let tracker = FakeTracker::default();
        let tickets = vec!["INTEL-1".into(), "INTEL-2".into()];
        let report = advance_tickets(&tracker, &tickets, "").unwrap();
        assert!(!report.has_failures());
        assert_eq!(
            tracker.transitioned(),
            [
                ("INTEL-1".into(), "11".into()),
                ("INTEL-2".into(), "11".into()),
            ]
        );
    }

    #[test]
    fn auth_error_fails_fast_without_trying_remaining_tickets() {
        let tracker = FakeTracker {
            fatal_on_ticket: Some((
                "INTEL-1".into(),
                JiraError::Auth {
                    url: "https://jira.example/rest/api/2/issue/INTEL-1".into(),
                    status: 401,
                    detail: "HTTP 401".into(),
                },
            )),
            ..FakeTracker::default()
        };
        let tickets = vec!["INTEL-1".into(), "INTEL-2".into(), "INTEL-3".into()];
        let err = advance_tickets(&tracker, &tickets, "done").unwrap_err();
        assert!(matches!(err, JiraError::Auth { status: 401, .. }));
        assert_eq!(tracker.requested(), ["INTEL-1"]);
        assert_eq!(tracker.transitioned(), []);
    }

    #[test]
    fn transport_error_fails_fast_without_trying_remaining_tickets() {
        let tracker = FakeTracker {
            fatal_on_ticket: Some((
                "INTEL-1".into(),
                JiraError::Transport {
                    url: "https://jira.example/rest/api/2/issue/INTEL-1".into(),
                    detail: "dns failed".into(),
                },
            )),
            ..FakeTracker::default()
        };
        let tickets = vec!["INTEL-1".into(), "INTEL-2".into(), "INTEL-3".into()];
        let err = advance_tickets(&tracker, &tickets, "done").unwrap_err();
        assert!(matches!(err, JiraError::Transport { .. }));
        assert_eq!(tracker.requested(), ["INTEL-1"]);
        assert_eq!(tracker.transitioned(), []);
    }

    #[test]
    fn followup_read_failure_after_transition_is_reported_as_moved_not_failed() {
        let tracker = FakeTracker {
            fail_after_transition_read: Some("INTEL-1".into()),
            ..FakeTracker::default()
        };
        let tickets = vec!["INTEL-1".into()];
        let report = advance_tickets(&tracker, &tickets, "done").unwrap();
        assert!(!report.has_failures());
        assert_eq!(tracker.transitioned(), [("INTEL-1".into(), "21".into())]);
        let record = &report.records()[0];
        let BatchOutcome::Succeeded(TicketAdvance::MovedUnknownAfter {
            before,
            transition,
            warning,
        }) = &record.outcome
        else {
            panic!("expected MovedUnknownAfter, got {record:?}");
        };
        assert_eq!(record.key, "INTEL-1");
        assert_eq!(before, "Open");
        assert_eq!(transition, "Resolve");
        assert!(warning.contains("HTTP 500 on follow-up read"), "{warning}");
    }

    #[test]
    fn streams_each_ticket_outcome_before_requesting_the_next_ticket() {
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let tracker = FakeTracker {
            events: Some(std::sync::Arc::clone(&events)),
            ..FakeTracker::default()
        };
        let tickets = vec!["INTEL-1".into(), "INTEL-2".into()];
        let report = advance_tickets_with(
            &tracker,
            &AdvanceRequest {
                tickets: &tickets,
                wanted_transition: "done",
                dry_run: false,
            },
            |record| {
                events
                    .lock()
                    .unwrap()
                    .push(format!("reported:{}", record.key));
            },
        )
        .unwrap();
        assert!(!report.has_failures());
        assert_eq!(
            *events.lock().unwrap(),
            [
                "request:INTEL-1",
                "request:INTEL-1",
                "reported:INTEL-1",
                "request:INTEL-2",
                "request:INTEL-2",
                "reported:INTEL-2",
            ]
        );
    }

    #[test]
    fn dry_run_resolves_without_calling_transition_or_second_ticket_read() {
        let tracker = FakeTracker::default();
        let tickets = vec!["INTEL-1".into(), "INTEL-2".into()];
        let report = advance_tickets_with(
            &tracker,
            &AdvanceRequest {
                tickets: &tickets,
                wanted_transition: "In Progress",
                dry_run: true,
            },
            |_| {},
        )
        .unwrap();
        assert!(!report.has_failures());
        assert_eq!(tracker.transitioned(), []);
        assert_eq!(tracker.requested(), ["INTEL-1", "INTEL-2"]);
        assert_eq!(
            report.records(),
            [
                would_advance("INTEL-1", "Open", "In Progress", "Start Progress"),
                would_advance("INTEL-2", "Open", "In Progress", "Start Progress"),
            ]
        );
    }

    #[test]
    fn dry_run_and_moved_unknown_count_as_succeeded_and_only_failed_tickets_fail() {
        let moved = FakeTracker {
            fail_after_transition_read: Some("INTEL-1".into()),
            ..FakeTracker::default()
        };
        let report = advance_tickets(&moved, &["INTEL-1".into()], "done").unwrap();
        assert_eq!(report.counts().succeeded, 1);
        assert_eq!(report.outcome(), vdp_core::Outcome::Success);

        let dry = FakeTracker::default();
        let request = AdvanceRequest {
            tickets: &["INTEL-1".into()],
            wanted_transition: "done",
            dry_run: true,
        };
        let report = advance_tickets_with(&dry, &request, |_| {}).unwrap();
        assert_eq!(report.counts().succeeded, 1);
        assert_eq!(report.outcome(), vdp_core::Outcome::Success);

        let report = advance_tickets(&dry, &["INTEL-1".into()], "Deploy").unwrap();
        assert_eq!(report.counts().failed, 1);
        assert_eq!(report.outcome(), vdp_core::Outcome::Failures);
    }
}
