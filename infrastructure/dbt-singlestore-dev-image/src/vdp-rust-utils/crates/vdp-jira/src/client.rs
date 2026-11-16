use std::fmt;

use base64::Engine as _;
use serde_json::{Value, json};
use ureq::Agent;
use vdp_secrets::{ExposeSecret, SecretString};

use crate::{IssueTracker, JiraError, Ticket, Transition};

/// Jira Cloud REST v2 with basic authentication (account + API token).
pub struct JiraClient {
    base_url: String,
    authorization: SecretString,
    agent: Agent,
    sprint_field: String,
}

impl fmt::Debug for JiraClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JiraClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl JiraClient {
    pub fn new(base_url: &str, user: &SecretString, token: &SecretString) -> Self {
        let credentials = format!("{}:{}", user.expose_secret(), token.expose_secret());
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            authorization: SecretString::from(format!("Basic {encoded}")),
            agent: Agent::config_builder()
                .proxy(ureq::Proxy::try_from_env())
                .build()
                .into(),
            sprint_field: "customfield_11221".into(),
        }
    }

    /// The custom field holding sprints (`customfield_11221` at the organisation).
    #[must_use]
    pub fn with_sprint_field(mut self, field: &str) -> Self {
        field.clone_into(&mut self.sprint_field);
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn issue_url(&self, key: &str) -> String {
        format!("{}/rest/api/2/issue/{key}", self.base_url)
    }

    fn get(&self, url: &str) -> Result<Value, JiraError> {
        self.agent
            .get(url)
            .header("Authorization", self.authorization.expose_secret())
            .header("Accept", "application/json")
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| JiraError::Request {
                url: url.to_owned(),
                detail: e.to_string(),
            })
    }
}

fn names(value: &Value, field: &str) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v[field].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A REST v2 issue as a [`Ticket`].
pub(crate) fn parse_ticket(key: &str, body: &Value, sprint_field: &str) -> Ticket {
    let fields = &body["fields"];
    let person = |field: &str| fields[field]["displayName"].as_str().map(str::to_owned);
    let sprints = fields[sprint_field].as_array();
    let sprint = sprints.and_then(|list| {
        list.iter()
            .find(|s| {
                s["state"]
                    .as_str()
                    .is_some_and(|st| st.eq_ignore_ascii_case("active"))
            })
            .or_else(|| list.first())
            .and_then(|s| s["name"].as_str().map(str::to_owned))
    });
    Ticket {
        key: body["key"].as_str().unwrap_or(key).to_owned(),
        summary: fields["summary"].as_str().unwrap_or_default().to_owned(),
        status: fields["status"]["name"]
            .as_str()
            .unwrap_or("Unknown")
            .to_owned(),
        fix_versions: names(&fields["fixVersions"], "name"),
        assignee: person("assignee"),
        reporter: person("reporter"),
        components: names(&fields["components"], "name"),
        sprint,
    }
}

impl IssueTracker for JiraClient {
    fn ticket(&self, key: &str) -> Result<Ticket, JiraError> {
        let url = format!(
            "{}?fields=summary,status,fixVersions,assignee,reporter,components,{}",
            self.issue_url(key),
            self.sprint_field
        );
        let body = self.get(&url)?;
        let fields = &body["fields"];
        if fields.is_null() {
            return Err(JiraError::Response {
                url,
                detail: "no fields".into(),
            });
        }
        Ok(parse_ticket(key, &body, &self.sprint_field))
    }

    fn transitions(&self, key: &str) -> Result<Vec<Transition>, JiraError> {
        let url = format!("{}/transitions", self.issue_url(key));
        let body = self.get(&url)?;
        Ok(body["transitions"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|t| Transition {
                        id: t["id"].as_str().unwrap_or_default().to_owned(),
                        name: t["name"].as_str().unwrap_or_default().to_owned(),
                        to: t["to"]["name"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn transition(&self, key: &str, transition_id: &str) -> Result<(), JiraError> {
        let url = format!("{}/transitions", self.issue_url(key));
        self.agent
            .post(&url)
            .header("Authorization", self.authorization.expose_secret())
            .header("Accept", "application/json")
            .send_json(json!({ "transition": { "id": transition_id } }))
            .map(|_| ())
            .map_err(|e| JiraError::Request {
                url,
                detail: e.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_fields_the_board_shows() {
        let body = json!({"key": "INTEL-1", "fields": {
            "summary": "Load X", "status": {"name": "Done"},
            "fixVersions": [{"name": "2026_DROP05"}, {"name": "2026_EDROP03"}],
            "assignee": {"displayName": "Jane"}, "reporter": null,
            "components": [{"name": "DBT"}],
            "customfield_11221": [{"name": "S1", "state": "closed"}, {"name": "S2", "state": "ACTIVE"}]
        }});
        let ticket = parse_ticket("INTEL-1", &body, "customfield_11221");
        assert_eq!(ticket.summary, "Load X");
        assert_eq!(ticket.assignee.as_deref(), Some("Jane"));
        assert_eq!(ticket.reporter, None);
        assert_eq!(ticket.components, ["DBT"]);
        assert_eq!(ticket.sprint.as_deref(), Some("S2"));
        assert_eq!(ticket.release().unwrap().name(), "2026_DROP05");
        assert_eq!(ticket.emergency_drops()[0].name(), "2026_EDROP03");
    }
}
