//! Sign-in with a personal GitLab token. Only people who may force-push main and the release branches may
//! release; the hub checks that through the GitLab API and then pushes as them (over HTTPS, with their token),
//! so GitLab itself enforces it. Tokens live in memory only.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vdp_git::{Identity, PushTarget};

use crate::error::HubError;

/// The GitLab project the hub releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLab {
    /// `https://git.example.com`.
    pub url: String,
    /// `vdp/dbt-vdp`.
    pub project: String,
    /// Where pushes go: `https://git.example.com/vdp/dbt-vdp.git`.
    pub push_url: String,
}

impl GitLab {
    /// From a clone URL: `git@host:group/project.git`, `ssh://git@host[:port]/group/project.git` or
    /// `https://host/group/project(.git)`.
    pub fn from_repo_url(repo_url: &str) -> Option<Self> {
        let repo_url = repo_url.trim();
        let (host, path) = if let Some(rest) = repo_url.strip_prefix("https://") {
            rest.split_once('/')?
        } else if let Some(rest) = repo_url.strip_prefix("ssh://") {
            let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
            let (host, path) = rest.split_once('/')?;
            (host.split(':').next()?, path)
        } else {
            let rest = repo_url.split_once('@').map_or(repo_url, |(_, r)| r);
            rest.split_once(':')?
        };
        let project = path
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_owned();
        if host.is_empty() || project.is_empty() {
            return None;
        }
        Some(Self {
            url: format!("https://{host}"),
            push_url: format!("https://{host}/{project}.git"),
            project,
        })
    }

    fn get(&self, token: &str, path: &str) -> Result<Value, HubError> {
        let url = format!("{}/api/v4{path}", self.url.trim_end_matches('/'));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into();
        let mut response = agent
            .get(&url)
            .header("PRIVATE-TOKEN", token)
            .call()
            .map_err(|e| HubError::Internal(format!("GitLab is not reachable ({url}): {e}")))?;
        let status = response.status().as_u16();
        let mut body = String::new();
        response
            .body_mut()
            .as_reader()
            .take(4 * 1024 * 1024)
            .read_to_string(&mut body)
            .map_err(|e| HubError::Internal(format!("GitLab answer: {e}")))?;
        match status {
            200..=299 => serde_json::from_str(&body)
                .map_err(|e| HubError::Internal(format!("GitLab answer: {e}"))),
            401 => Err(HubError::Unauthorized(
                "GitLab refused the token (wrong, expired or revoked)".into(),
            )),
            403 | 404 => Err(HubError::Forbidden(format!(
                "the token has no access to {} (needs the read_api and write_repository scopes)",
                self.project
            ))),
            _ => Err(HubError::Internal(format!(
                "GitLab answered {status} for {path}"
            ))),
        }
    }

    /// Who the token belongs to.
    pub fn user(&self, token: &str) -> Result<GitLabUser, HubError> {
        let user = self.get(token, "/user")?;
        let text = |v: &Value, key: &str| v[key].as_str().unwrap_or_default().to_owned();
        let email = Some(text(&user, "commit_email"))
            .filter(|e| e.contains('@'))
            .or_else(|| Some(text(&user, "email")).filter(|e| e.contains('@')))
            .or_else(|| Some(text(&user, "public_email")).filter(|e| e.contains('@')))
            .unwrap_or_default();
        Ok(GitLabUser {
            id: user["id"].as_u64().unwrap_or(0),
            username: text(&user, "username"),
            name: Some(text(&user, "name"))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| text(&user, "username")),
            email,
        })
    }

    /// What `user` may do on each branch: `(label, a branch name the rule is tested with)`.
    pub fn rights(
        &self,
        token: &str,
        user: &GitLabUser,
        branches: &[(String, String)],
    ) -> Result<Vec<BranchRight>, HubError> {
        let encoded = self.project.replace('/', "%2F");
        let project = self.get(token, &format!("/projects/{encoded}"))?;
        let access = |kind: &str| {
            project["permissions"][kind]["access_level"]
                .as_u64()
                .unwrap_or(0)
        };
        let level = access("project_access").max(access("group_access"));
        let protected = self.get(
            token,
            &format!(
                "/projects/{}/protected_branches?per_page=100",
                project["id"]
            ),
        )?;
        let protected = protected.as_array().cloned().unwrap_or_default();
        Ok(branches
            .iter()
            .map(|(label, sample)| branch_right(level, user.id, &protected, sample, label))
            .collect())
    }

    /// The newest pipeline GitLab ran for `sha` in this project (`None`: none yet).
    pub fn pipeline(&self, token: &str, sha: &str) -> Result<Option<Pipeline>, HubError> {
        if sha.len() < 7 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(HubError::BadRequest(format!("`{sha}` is not a commit id")));
        }
        let encoded = self.project.replace('/', "%2F");
        let list = self.get(
            token,
            &format!("/projects/{encoded}/pipelines?sha={sha}&per_page=1"),
        )?;
        Ok(list.as_array().and_then(|l| l.first()).map(|p| Pipeline {
            id: p["id"].as_u64().unwrap_or(0),
            status: p["status"].as_str().unwrap_or_default().to_owned(),
            branch: p["ref"].as_str().unwrap_or_default().to_owned(),
            web_url: p["web_url"].as_str().unwrap_or_default().to_owned(),
        }))
    }

    /// Who the token belongs to, and whether they may release (force-push `main` and the release branches).
    pub fn sign_in(
        &self,
        token: &str,
        main: &str,
        release_prefix: &str,
    ) -> Result<SessionUser, HubError> {
        let user = self.user(token)?;
        let rights = self.rights(
            token,
            &user,
            &[
                (main.to_owned(), main.to_owned()),
                (
                    format!("{release_prefix}*"),
                    format!("{release_prefix}2099_DROP99"),
                ),
            ],
        )?;
        let mut reasons: Vec<String> = rights
            .iter()
            .filter(|r| !r.force)
            .filter_map(|r| r.reason.clone())
            .collect();
        if user.email.is_empty() {
            reasons
                .push("your GitLab account has no commit email: set one, commits need it".into());
        }
        Ok(SessionUser {
            username: user.username,
            name: user.name,
            email: user.email,
            can_release: reasons.is_empty(),
            reasons,
            rights: BTreeMap::new(),
        })
    }
}

/// A CI pipeline (`status`: created, pending, running, success, failed, canceled, skipped, manual, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Pipeline {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub id: u64,
    pub status: String,
    pub branch: String,
    pub web_url: String,
}

/// A GitLab account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLabUser {
    pub id: u64,
    pub username: String,
    pub name: String,
    pub email: String,
}

/// What a person may do with one branch (or branch pattern).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BranchRight {
    /// `CBT`, `main`, `feature/*`.
    pub branch: String,
    pub push: bool,
    pub force: bool,
    /// Why not (push, or force push), when not.
    pub reason: Option<String>,
}

/// `*` anywhere in a protected-branch name matches anything (GitLab's wildcards).
fn wildcard(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            let Some(after) = rest.strip_prefix(part) else {
                return false;
            };
            rest = after;
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            let Some(at) = rest.find(part) else {
                return false;
            };
            rest = &rest[at + part.len()..];
        }
    }
    true
}

/// What the user may do on `branch` under GitLab's rules: an unprotected branch takes pushes and force pushes
/// from Developers up; a protected one from its push levels (or named users), force only when allowed.
fn branch_right(
    level: u64,
    user_id: u64,
    protected: &[Value],
    branch: &str,
    label: &str,
) -> BranchRight {
    const DEVELOPER: u64 = 30;
    let right = |push: bool, force: bool, reason: Option<String>| BranchRight {
        branch: label.to_owned(),
        push,
        force,
        reason,
    };
    let rule = protected
        .iter()
        .find(|p| p["name"].as_str().is_some_and(|n| n == branch))
        .or_else(|| {
            protected
                .iter()
                .find(|p| p["name"].as_str().is_some_and(|n| wildcard(n, branch)))
        });
    let Some(rule) = rule else {
        return if level >= DEVELOPER {
            right(true, true, None)
        } else {
            right(
                false,
                false,
                Some(format!(
                    "{label}: Developer access or more is needed to push"
                )),
            )
        };
    };
    let may_push = rule["push_access_levels"].as_array().is_some_and(|levels| {
        levels.iter().any(|l| {
            let required = l["access_level"].as_u64().unwrap_or(0);
            (required > 0 && l["user_id"].is_null() && l["group_id"].is_null() && level >= required)
                || l["user_id"].as_u64() == Some(user_id)
        })
    });
    if !may_push {
        return right(
            false,
            false,
            Some(format!(
                "{label} is protected: you are not allowed to push to it"
            )),
        );
    }
    if rule["allow_force_push"].as_bool() != Some(true) {
        return right(
            true,
            false,
            Some(format!(
                "{label} is protected without \"Allowed to force push\": force push (needed to release) is refused"
            )),
        );
    }
    right(true, true, None)
}

/// `None` when the user may force-push `branch`; otherwise why not.
#[cfg(test)]
fn force_push_refused(
    level: u64,
    user_id: u64,
    protected: &[Value],
    branch: &str,
    label: &str,
) -> Option<String> {
    let right = branch_right(level, user_id, protected, branch, label);
    (!right.force).then(|| right.reason.unwrap_or_default())
}

/// Who signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SessionUser {
    pub username: String,
    pub name: String,
    pub email: String,
    /// May force-push main and the release branches: may release, patch and promote.
    pub can_release: bool,
    /// Why not, when not.
    pub reasons: Vec<String>,
    /// Per extension (by its URL segment, e.g. `vaultspeed`) and per branch of its repository: what this person
    /// may push. Serialised next to the other fields, so the UI reads `user.vaultspeed`.
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(flatten))]
    pub rights: BTreeMap<String, Vec<BranchRight>>,
}

impl SessionUser {
    /// The right of an extension for `branch` (exact names first, then patterns such as `feature/*`).
    pub fn right(&self, extension: &str, branch: &str) -> Option<&BranchRight> {
        let rights = self.rights.get(extension)?;
        rights
            .iter()
            .find(|r| r.branch == branch)
            .or_else(|| rights.iter().find(|r| wildcard(&r.branch, branch)))
    }
}

/// A signed-in person: what jobs run as.
#[derive(Clone)]
pub struct Session {
    pub user: SessionUser,
    token: String,
    expires: Instant,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("user", &self.user)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn identity(&self) -> Identity {
        Identity {
            name: self.user.name.clone(),
            email: self.user.email.clone(),
        }
    }

    /// The person's token, for GitLab API calls made on their behalf.
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub fn push_target(&self, gitlab: &GitLab) -> PushTarget {
        PushTarget::Url {
            url: gitlab.push_url.clone(),
            token: self.token.clone(),
        }
    }
}

/// The sessions of this process (memory only: a restart signs everyone out).
#[derive(Debug, Default)]
pub struct Sessions {
    map: Mutex<HashMap<String, Session>>,
}

/// A session lasts this long.
pub const SESSION_HOURS: u64 = 12;

fn random_id() -> String {
    let mut bytes = [0u8; 24];
    let filled = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if filled.is_err() {
        // Not a Unix system: time and address entropy (never used in the container).
        let seed =
            Instant::now().elapsed().as_nanos() ^ (std::ptr::from_ref(&bytes) as usize as u128);
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = u8::try_from((seed >> (i % 16 * 8)) & 0xff).unwrap_or(0);
        }
    }
    bytes.iter().fold(String::with_capacity(48), |mut id, b| {
        let _ = write!(id, "{b:02x}");
        id
    })
}

impl Sessions {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Session>> {
        self.map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn create(&self, user: SessionUser, token: String) -> String {
        let id = random_id();
        let mut map = self.lock();
        map.retain(|_, s| s.expires > Instant::now());
        map.insert(
            id.clone(),
            Session {
                user,
                token,
                expires: Instant::now() + Duration::from_secs(SESSION_HOURS * 3600),
            },
        );
        id
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        let map = self.lock();
        map.get(id).filter(|s| s.expires > Instant::now()).cloned()
    }

    pub fn remove(&self, id: &str) {
        self.lock().remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gitlab_from_repo_urls() {
        let expected = GitLab {
            url: "https://git.example.com".into(),
            project: "vdp/dbt-vdp".into(),
            push_url: "https://git.example.com/vdp/dbt-vdp.git".into(),
        };
        for url in [
            "git@git.example.com:vdp/dbt-vdp.git",
            "ssh://git@git.example.com:2222/vdp/dbt-vdp.git",
            "https://git.example.com/vdp/dbt-vdp",
            "https://git.example.com/vdp/dbt-vdp.git",
        ] {
            assert_eq!(
                GitLab::from_repo_url(url).as_ref(),
                Some(&expected),
                "{url}"
            );
        }
        assert!(GitLab::from_repo_url("/tmp/origin.git").is_none());
    }

    #[test]
    fn force_push_rights() {
        let protected = [
            json!({"name": "main", "allow_force_push": true,
                   "push_access_levels": [{"access_level": 40, "user_id": null, "group_id": null}]}),
            json!({"name": "release/*", "allow_force_push": false,
                   "push_access_levels": [{"access_level": 40, "user_id": null, "group_id": null}]}),
        ];
        assert_eq!(force_push_refused(40, 1, &protected, "main", "main"), None);
        assert!(force_push_refused(30, 1, &protected, "main", "main").is_some());
        let release = force_push_refused(40, 1, &protected, "release/2099_DROP99", "release/*");
        assert!(release.unwrap().contains("force push"));
        assert_eq!(
            force_push_refused(30, 1, &protected, "feature/x", "feature/*"),
            None
        );
        assert!(force_push_refused(20, 1, &protected, "feature/x", "feature/*").is_some());
        let personal = [json!({"name": "main", "allow_force_push": true,
            "push_access_levels": [{"access_level": 0, "user_id": 7, "group_id": null}]})];
        assert_eq!(force_push_refused(10, 7, &personal, "main", "main"), None);
        assert!(wildcard("release/*", "release/2026_DROP25"));
        assert!(!wildcard("release/*", "main"));
    }
}
