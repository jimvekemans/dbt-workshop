//! The HTTP layer: `/api/v1` JSON routes over [`Hub`], and optionally the built frontend.

use std::io::Read;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use tiny_http::{Header, Request, Response, Server};

use crate::api::{
    AnalysisCreate, AnnouncementSend, ApiError, CloneRequest, GitIdentity, PatchCreate,
    PromotionCreate, ReleaseCreate, SelectionUpdate, SignIn,
};
use crate::auth::SESSION_HOURS;
use crate::error::HubError;
use crate::extension::ExtensionRequest;
use crate::hub::Hub;
use vdp_release::Resolution;

/// The API prefix.
pub const API: &str = "/api/v1";
const MAX_BODY: u64 = 1024 * 1024;

/// A response before it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// A `Set-Cookie` value (the session cookie).
    pub set_cookie: Option<String>,
}

/// The session cookie.
pub const SESSION_COOKIE: &str = "vdp_session";

impl Reply {
    pub fn json<T: Serialize>(status: u16, value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => Self {
                status,
                content_type: "application/json",
                body,
                set_cookie: None,
            },
            Err(e) => Self::error(&HubError::Internal(format!("serialising: {e}"))),
        }
    }

    pub fn error(error: &HubError) -> Self {
        Self::json(
            error.status(),
            &ApiError {
                code: error.code().into(),
                error: error.to_string(),
            },
        )
    }

    pub fn from<T: Serialize>(status: u16, result: Result<T, HubError>) -> Self {
        match result {
            Ok(value) => Self::json(status, &value),
            Err(error) => Self::error(&error),
        }
    }
}

fn hex(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// `%2F` → `/`, `+` stays (paths) or becomes a space (`query`).
fn percent_decode(text: &str, query: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                match (
                    bytes.get(i + 1).copied().and_then(hex),
                    bytes.get(i + 2).copied().and_then(hex),
                ) {
                    (Some(h), Some(l)) => {
                        out.push(h * 16 + l);
                        i += 3;
                        continue;
                    }
                    _ => out.push(b'%'),
                }
            }
            b'+' if query => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A query parameter, decoded.
pub(crate) fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (percent_decode(key, true) == name).then(|| percent_decode(value, true))
    })
}

pub(crate) fn parse_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, HubError> {
    let body = if body.iter().all(u8::is_ascii_whitespace) {
        b"{}".as_slice()
    } else {
        body
    };
    serde_json::from_slice(body).map_err(|e| HubError::BadRequest(format!("request body: {e}")))
}

/// Routes one API request. `path` is below [`API`], undecoded; `query` without the `?`; `session` the session
/// cookie. Everything but `/status` and `/session` needs a sign-in (with GitLab sign-in on); what changes the
/// clone or pushes needs release rights.
#[allow(clippy::too_many_lines, reason = "one route table")]
pub fn route(
    hub: &Arc<Hub>,
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    session: Option<&str>,
) -> Reply {
    let decoded = percent_decode(path, false);
    let path = decoded.trim_end_matches('/');
    let mut segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    // `/dbt/...` is the dbt side's own prefix; the unprefixed routes are the same.
    if segments.first() == Some(&"dbt") {
        segments.remove(0);
    }
    let public = matches!(
        (method, segments.as_slice()),
        ("GET", ["status"]) | (_, ["session"])
    );
    if !public && let Err(error) = hub.actor(session, false) {
        return Reply::error(&error);
    }
    if let [first, rest @ ..] = segments.as_slice()
        && let Some(extension) = hub.extension(first)
    {
        return extension.handle(&ExtensionRequest {
            method,
            segments: rest,
            query,
            body,
            session,
            host: &hub.host_of(extension.as_ref()),
        });
    }
    let writer = || hub.actor(session, true);
    match (method, segments.as_slice()) {
        ("GET", ["status"]) => Reply::json(200, &hub.status(session)),
        ("POST", ["session"]) => {
            match parse_body::<SignIn>(body).and_then(|s| hub.sign_in(&s.token)) {
                Ok((id, user)) => {
                    let mut reply = Reply::json(200, &user);
                    reply.set_cookie = Some(format!(
                        "{SESSION_COOKIE}={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
                        SESSION_HOURS * 3600
                    ));
                    reply
                }
                Err(error) => Reply::error(&error),
            }
        }
        ("GET", ["session"]) => match hub.session_user(session) {
            Some(user) => Reply::json(200, &user),
            None => Reply::error(&HubError::Unauthorized("not signed in".into())),
        },
        ("DELETE", ["session"]) => {
            hub.sign_out(session);
            Reply {
                status: 204,
                content_type: "text/plain",
                body: Vec::new(),
                set_cookie: Some(format!(
                    "{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
                )),
            }
        }
        ("PUT", ["identity"]) => Reply::from(
            200,
            parse_body::<GitIdentity>(body).and_then(|i| hub.set_identity(&i)),
        ),
        ("POST", ["repo", "clone"]) => Reply::from(
            202,
            writer()
                .and_then(|_| parse_body::<CloneRequest>(body).and_then(|r| hub.clone_repo(&r))),
        ),
        ("POST", ["repo", "sync"]) => Reply::from(202, hub.sync()),
        ("POST", ["repo", "repair"]) => Reply::from(202, writer().and_then(|_| hub.repair())),
        ("POST", ["repo", "reclone"]) => Reply::from(
            202,
            writer().and_then(|_| parse_body::<CloneRequest>(body).and_then(|r| hub.reclone(&r))),
        ),
        ("GET", ["release-branches"]) => Reply::from(200, hub.release_branches()),
        // Branch names contain slashes: /release-branches/release/2026_DROP24/promotion.
        ("GET", ["release-branches", name @ .., "promotion"]) if !name.is_empty() => {
            Reply::from(200, hub.promotion(&name.join("/")))
        }
        ("GET", ["changes"]) => Reply::from(
            200,
            hub.changes(
                &query_param(query, "from").unwrap_or_default(),
                &query_param(query, "to").unwrap_or_default(),
            ),
        ),
        ("GET", ["changes", "promotion"]) => Reply::from(
            200,
            hub.promotion_changes(&query_param(query, "branch").unwrap_or_default()),
        ),
        ("GET", ["changes", "file"]) => Reply::from(
            200,
            hub.change_file(
                &query_param(query, "from").unwrap_or_default(),
                &query_param(query, "to").unwrap_or_default(),
                &query_param(query, "path").unwrap_or_default(),
            ),
        ),
        ("GET", ["analyses"]) => Reply::json(200, &hub.analyses()),
        ("POST", ["analyses"]) => Reply::from(
            202,
            parse_body::<AnalysisCreate>(body).and_then(|c| hub.create_analysis(&c)),
        ),
        ("GET", ["analyses", id]) => Reply::from(200, hub.analysis(id)),
        ("PUT", ["analyses", id, "selection"]) => Reply::from(
            200,
            parse_body::<SelectionUpdate>(body).and_then(|s| hub.set_selection(id, &s)),
        ),
        ("GET", ["analyses", id, "announcement"]) => Reply::from(
            200,
            hub.announcement(id, query_param(query, "branch").as_deref()),
        ),
        ("POST", ["releases"]) => Reply::from(
            202,
            writer().and_then(|actor| {
                parse_body::<ReleaseCreate>(body).and_then(|c| hub.create_release(&c, actor))
            }),
        ),
        ("POST", ["patches"]) => Reply::from(
            202,
            writer().and_then(|actor| {
                parse_body::<PatchCreate>(body).and_then(|c| hub.create_patch(&c, actor))
            }),
        ),
        ("POST", ["promotions"]) => Reply::from(
            202,
            writer().and_then(|actor| {
                parse_body::<PromotionCreate>(body).and_then(|c| hub.create_promotion(&c, actor))
            }),
        ),
        ("POST", ["announcements"]) => Reply::from(
            200,
            parse_body::<AnnouncementSend>(body).and_then(|s| hub.send_announcement(&s)),
        ),
        ("GET", ["jobs"]) => Reply::json(200, &hub.jobs()),
        ("POST", ["jobs", id, "resolution"]) => Reply::from(
            200,
            writer().and_then(|actor| {
                parse_body::<Resolution>(body)
                    .and_then(|r| hub.resolve(id, r, &actor).and_then(|()| hub.job(id, 0)))
            }),
        ),
        ("GET", ["jobs", id, "conflict"]) => match query_param(query, "path") {
            Some(path) => Reply::from(200, hub.conflict_versions(id, &path)),
            None => Reply::error(&HubError::BadRequest("path is required".into())),
        },
        ("GET", ["jobs", id]) => {
            let after = query_param(query, "after")
                .and_then(|a| a.parse().ok())
                .unwrap_or(0);
            Reply::from(200, hub.job(id, after))
        }
        _ => Reply::error(&HubError::NotFound(format!("{method} {API}{path}"))),
    }
}

/// The session cookie of a request.
fn session_of(request: &Request) -> Option<String> {
    request
        .headers()
        .iter()
        .filter(|h| h.field.equiv("Cookie"))
        .flat_map(|h| {
            h.value
                .as_str()
                .split(';')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .find_map(|pair| {
            pair.strip_prefix(&format!("{SESSION_COOKIE}="))
                .map(str::to_owned)
        })
        .filter(|id| !id.is_empty())
}

/// How the server behaves besides the API.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// `Access-Control-Allow-Origin`; `None`: same origin only.
    pub cors_origin: Option<String>,
    /// The built frontend (a single-page app: unknown paths get `index.html`).
    pub ui_dir: Option<PathBuf>,
}

/// The listening server.
pub struct HubServer {
    server: Arc<Server>,
    options: ServeOptions,
}

impl std::fmt::Debug for HubServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubServer")
            .field("addr", &self.local_addr())
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

/// A file of the frontend, or `index.html` for client-side routes.
fn static_file(root: &Path, url_path: &str) -> Option<(PathBuf, bool)> {
    let relative = percent_decode(url_path, false);
    let relative = Path::new(relative.trim_start_matches('/'));
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let index = root.join("index.html");
    if relative.as_os_str().is_empty() {
        return index.is_file().then_some((index, false));
    }
    let candidate = root.join(relative);
    if candidate.is_file() {
        let immutable = relative.starts_with("_app/immutable");
        return Some((candidate, immutable));
    }
    let html = candidate.with_extension("html");
    if relative.extension().is_none() && html.is_file() {
        return Some((html, false));
    }
    if relative.extension().is_some() {
        return None;
    }
    index.is_file().then_some((index, false))
}

impl HubServer {
    pub fn bind(addr: &str, options: ServeOptions) -> Result<Self, HubError> {
        let server = Server::http(addr)
            .map_err(|e| HubError::Internal(format!("cannot listen on {addr}: {e}")))?;
        Ok(Self {
            server: Arc::new(server),
            options,
        })
    }

    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.server.server_addr().to_ip()
    }

    /// Stops [`HubServer::run`] from another thread.
    pub fn stopper(&self) -> impl Fn() + Send + Sync + 'static {
        let server = Arc::clone(&self.server);
        move || server.unblock()
    }

    /// Serves until [`HubServer::stopper`] is called; each request on its own thread (an announcement waits
    /// for SMTP, git queries for git).
    pub fn run(&self, hub: &Arc<Hub>) {
        for request in self.server.incoming_requests() {
            let hub = Arc::clone(hub);
            let options = self.options.clone();
            let spawned = std::thread::Builder::new()
                .name("http".into())
                .spawn(move || handle(&hub, &options, request));
            if let Err(e) = spawned {
                tracing::error!("cannot handle a request: {e}");
            }
        }
    }
}

fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

#[allow(
    clippy::too_many_lines,
    reason = "one request: an API call or a static file"
)]
fn handle(hub: &Arc<Hub>, options: &ServeOptions, mut request: Request) {
    let method = request.method().to_string().to_uppercase();
    let url = request.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let cors: Vec<Header> = options
        .cors_origin
        .iter()
        .flat_map(|origin| {
            [
                header("Access-Control-Allow-Origin", origin),
                header(
                    "Access-Control-Allow-Methods",
                    "GET, POST, PUT, DELETE, OPTIONS",
                ),
                header("Access-Control-Allow-Headers", "Content-Type"),
                header("Access-Control-Allow-Credentials", "true"),
                header("Vary", "Origin"),
            ]
        })
        .flatten()
        .collect();
    let respond = |request: Request, reply: Reply, extra: Vec<Header>| {
        let mut response = Response::from_data(reply.body).with_status_code(reply.status);
        for h in cors.iter().cloned().chain(extra) {
            response = response.with_header(h);
        }
        if let Some(h) = header("Content-Type", reply.content_type) {
            response = response.with_header(h);
        }
        if let Some(h) = reply
            .set_cookie
            .as_deref()
            .and_then(|cookie| header("Set-Cookie", cookie))
        {
            response = response.with_header(h);
        }
        if let Err(e) = request.respond(response) {
            tracing::debug!("client went away: {e}");
        }
    };

    let session = session_of(&request);
    if let Some(api_path) = path.strip_prefix(API) {
        if method == "OPTIONS" {
            let reply = Reply {
                status: 204,
                content_type: "text/plain",
                body: Vec::new(),
                set_cookie: None,
            };
            return respond(request, reply, Vec::new());
        }
        let mut body = Vec::new();
        let read = request
            .as_reader()
            .take(MAX_BODY + 1)
            .read_to_end(&mut body);
        let reply = match read {
            Err(e) => Reply::error(&HubError::BadRequest(format!("reading the body: {e}"))),
            Ok(_) if body.len() as u64 > MAX_BODY => {
                Reply::error(&HubError::BadRequest("request body too large".into()))
            }
            Ok(_) => route(hub, &method, api_path, query, &body, session.as_deref()),
        };
        if reply.status >= 500 {
            tracing::error!(%method, %path, status = reply.status, "request failed");
        } else {
            tracing::debug!(%method, %path, status = reply.status);
        }
        let no_store = header("Cache-Control", "no-store").into_iter().collect();
        return respond(request, reply, no_store);
    }

    let file = options
        .ui_dir
        .as_deref()
        .filter(|_| method == "GET" || method == "HEAD")
        .and_then(|root| static_file(root, path));
    match file.and_then(|(file, immutable)| std::fs::read(&file).ok().map(|b| (file, b, immutable)))
    {
        Some((file, body, immutable)) => {
            let cache = if immutable {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            let reply = Reply {
                status: 200,
                content_type: content_type(&file),
                body,
                set_cookie: None,
            };
            respond(
                request,
                reply,
                header("Cache-Control", cache).into_iter().collect(),
            );
        }
        None => respond(
            request,
            Reply::error(&HubError::NotFound(path.to_owned())),
            Vec::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding() {
        assert_eq!(
            percent_decode("release%2F2026_DROP24", false),
            "release/2026_DROP24"
        );
        assert_eq!(percent_decode("a+b%20c%", true), "a b c%");
        assert_eq!(percent_decode("%zz", false), "%zz");
        assert_eq!(
            query_param("after=3&branch=release%2Fx", "branch").as_deref(),
            Some("release/x")
        );
        assert_eq!(query_param("after=3", "branch"), None);
    }

    #[test]
    fn static_files_stay_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html>").unwrap();
        std::fs::create_dir_all(dir.path().join("_app/immutable")).unwrap();
        std::fs::write(dir.path().join("_app/immutable/a.js"), "x").unwrap();
        let root = dir.path();
        assert_eq!(static_file(root, "/").unwrap().0, root.join("index.html"));
        assert_eq!(
            static_file(root, "/release/new").unwrap().0,
            root.join("index.html")
        );
        assert!(static_file(root, "/_app/immutable/a.js").unwrap().1);
        assert!(static_file(root, "/_app/missing.js").is_none());
        assert!(static_file(root, "/../etc/passwd").is_none());
        assert!(static_file(root, "/%2e%2e/etc/passwd").is_none());
    }
}
