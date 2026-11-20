# HTTP API reference

The server is `http::HubServer` (tiny_http, plain HTTP, a thread per request); the routes are the match in `http::route`
(`crates/vdp-release-hub/src/http.rs`), which is a pure function of `(method, path, query, body, session)` and is what tests call.
Request and response bodies are in `api.rs` unless a type name says otherwise; the Rust and TypeScript names are the same.
Behaviour below was read from the code; the main routes were also tried by hand against a throwaway hub with `auth = "none"` on a local bare repository.

## Request handling

| Topic | Behaviour |
|---|---|
| Prefix | `/api/v1` (`http::API`). Everything else is a static file (GET/HEAD with a `ui_dir`) or a 404 `{"code":"not_found",...}`. A path such as `/api/v1x` is routed as an API path too (its remainder `x` is a 404). |
| `dbt` prefix | `/api/v1/dbt/<route>` is `/api/v1/<route>` (the first segment `dbt` is dropped). |
| Path | Percent-decoded, trailing `/` trimmed, split at `/`. `/api/v1/` (empty remainder) is a 404. |
| Query | `key=value&...`, keys and values percent-decoded, `+` is a space. A missing parameter is the empty string (`from`, `to`, `branch`, `path`) except where noted. |
| Body | JSON; an empty or whitespace body counts as `{}`; request types are `deny_unknown_fields` (400 `request body: unknown field ...`), except `GitIdentity` (`PUT /identity`) and `Resolution` (`POST /jobs/<id>/resolution`), which ignore unknown fields; more than 1 MiB is a 400 `request body too large`. Only read below `/api/v1`. |
| Session | The `vdp_session` cookie (any `Cookie` header, `;`-separated). Set by `POST /session` as `vdp_session=<48 hex>; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200`, cleared by `DELETE /session` (`Max-Age=0`). |
| `OPTIONS` | Any `OPTIONS` below `/api/v1` answers 204 at once, no sign-in needed. |
| CORS | With `--cors-origin`/`cors_origin`: `Access-Control-Allow-Origin: <origin>`, `-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS`, `-Allow-Headers: Content-Type`, `-Allow-Credentials: true`, `Vary: Origin`, on every response including static files. Without it none of these. |
| Caching | API replies: `Cache-Control: no-store`. Static: `no-cache`; files under `_app/immutable/` `public, max-age=31536000, immutable`. |
| Static files | Only with `ui_dir`, only GET and HEAD, never outside the root (`..` and `%2e%2e` give 404). `/` is `index.html`; a path with a file that exists is that file; without an extension it tries `<path>.html`, then `index.html` (client-side routes); a missing path with an extension is a 404. |
| Order of checks | (1) a route that is not `GET /status` or any method on `/session` needs `hub.actor(session, false)`: with `auth = "gitlab"` a valid session (401 `sign in with your GitLab token`), with `auth = "none"` the clone's identity read (500 only if git cannot read its config); (2) a matching extension segment is dispatched; (3) the core `match`, where *rights* routes call `hub.actor(session, true)` (403 `you may not release: <reasons>`). |

## Routes

*public*: no sign-in. *sign-in*: a session when `auth = "gitlab"`, nothing when `auth = "none"`. *rights*: sign-in and `can_release`
(`auth = "none"`: nothing). A job answer is a `Job` with status 202 and the steps so far (usually none).

| Method, path | Needs | Request | Success | Errors besides 401/403 |
|---|---|---|---|---|
| `GET /status` | public | | 200 `Status` | |
| `POST /session` | public | `SignIn {token}` | 200 `SessionUser` + cookie | 400 sign-in is off / a token is required; 401 GitLab refused the token; 403 no access to the project (needs `read_api`, `write_repository`); 500 GitLab unreachable |
| `GET /session` | public | | 200 `SessionUser` | 401 `not signed in` |
| `DELETE /session` | public | | 204, cookie cleared | |
| `PUT /identity` | sign-in | `GitIdentity {name, email}` | 200 `GitIdentity` (trimmed) | 400 when `auth = "gitlab"`, or name/email blank or invalid; 409 no clone |
| `POST /repo/clone` | rights | `CloneRequest {url?}` | 202 `Job` (kind `clone`) | 400 no URL; 409 already cloned (`sync it instead`) or the directory is not empty; 409 busy |
| `POST /repo/sync` | sign-in | | 202 `Job` (`sync`) | 409 no clone; 409 busy |
| `POST /repo/repair` | rights | | 202 `Job` (`repair`) | 409 no clone; 409 busy |
| `POST /repo/reclone` | rights | `CloneRequest` | 202 `Job` (`clone`, title `re-clone <url>`) | 409 busy. URL: the body's, else the clone's `origin`, else `repo_url` |
| `GET /release-branches` | sign-in | | 200 `ReleaseBranch[]` (`vdp-release`) | 409 no clone |
| `GET /release-branches/<branch>/promotion` | sign-in | branch name with its slashes in the path | 200 `Detection` (`kind`, `reason`, `nothing_to_promote`, `rollback_from`, `main_head`) | 409 no clone; 400 not a release branch; 404 `branch ... on origin` |
| `GET /changes` | sign-in | `?from=&to=`: a 40-hex commit id or `origin/<branch>` | 200 `FileChanges` | 409 no clone; 400 not a commit id or `origin/<branch>` |
| `GET /changes/promotion` | sign-in | `?branch=` | 200 `FileChanges` (new release: main to branch head; patch: main to the merge result, with `conflicts`) | as `promotion` |
| `GET /changes/file` | sign-in | `?from=&to=&path=` | 200 `FileDiff` (cut at 256 KiB) | 400 path is required / bad ref; 409 no clone |
| `GET /analyses` | sign-in | | 200 `AnalysisSummary[]`, newest first | |
| `POST /analyses` | sign-in | `AnalysisCreate` | 202 `Job` (`analysis`); result `{type: "analysis", analysis_id}` | 409 no clone; 400 see below; 409 busy |
| `GET /analyses/<id>` | sign-in | | 200 `AnalysisView` | 404 |
| `PUT /analyses/<id>/selection` | sign-in | `SelectionUpdate` | 200 `AnalysisView`, `version` bumped | 404; 409 the `version` is not current (`reload it`); 400 unknown or unselectable ticket |
| `GET /analyses/<id>/announcement` | sign-in | `?branch=` (default the analysis' branch) | 200 `Announcement` | 404 |
| `POST /releases` | rights | `ReleaseCreate` | 202 `Job` (`release`) | see "Executing" |
| `POST /patches` | rights | `PatchCreate` | 202 `Job` (`patch`) | see "Executing" |
| `POST /promotions` | rights | `PromotionCreate` | 202 `Job` (`promotion`) | 409 no clone / no git identity; 400 not a release branch; 404 not on origin; scope refusal; 409 busy |
| `POST /announcements` | sign-in | `AnnouncementSend` | 200 `AnnouncementSent` | 404 analysis; 400 `recipients` missing (`missing field`) or empty, or an invalid address when mail is really sent; 500 SMTP |
| `GET /jobs` | sign-in | | 200 `Job[]`, newest first, without `steps` | |
| `GET /jobs/<id>` | sign-in | `?after=<seq>` (not a number: 0) | 200 `Job` with the steps whose `seq` is greater | 404 |
| `POST /jobs/<id>/resolution` | rights | `Resolution` | 200 `Job` | 404; 409 not waiting / no git identity; 400 a path that is not in the conflict |
| `GET /jobs/<id>/conflict` | sign-in | `?path=` | 200 `ConflictVersionsView` | 400 path missing / not a conflicting file; 409 not waiting; 500 git |
| `/<segment>/...` | sign-in | | the extension's reply | the extension's |

`POST /analyses` 400s: a patch without `compare` (the release branch), a branch that is not `<branch_prefix>*` or is not one of the newest two (scope check),
a missing or invalid `fix_version` (`^\d{4}_E?DROP\d{1,2}$`; a patch defaults it to the branch suffix). A failed `git fetch` fails the job (an analysis of stale refs would be
reviewed as if current); a missing Jira connection only logs a warning.

**Executing** (`POST /releases`, `/patches`): checks in this order: clone exists (409), the actor has a complete git identity (409
`set the git identity ... first`), the analysis exists (404), is of the right kind (400), is not stale (409 `stale analysis: <ref> or <ref> moved since analysis <id>; analyse again`),
the branch is a release branch (400), the scope check allows it (the newest two only; the hub has no override), then a job starts (409 busy). Failures of the flow itself end the job as
`failed` with an `error`.

## Types

**`Status`** (`GET /status`): `version`; `auth` (`gitlab` | `none`); `user` (`SessionUser` | null); `repo` (`RepoStatus`); `identity` (`GitIdentity` | null);
`jira` and `smtp` (`Readiness {ready, detail}`); `active_job` (`Job` without steps | null); `defaults` (`Defaults {repo_url, source, main, branch_prefix, ticket_prefix, recipients}`);
plus one key per extension, its `summary()`. Public: it reveals the clone's directory and `origin` URL.

**`RepoStatus`**: `dir`, `cloned`, `origin_url`, `head`, `dirty`, `last_fetch` (ms, from `.git/FETCH_HEAD`), `health: HealthProblem[]` (`{code, detail}`;
codes `broken`, `in_progress`, `index_lock`, `dirty`, `local_branches`; empty while a job runs).

**`SessionUser`**: `username`, `name`, `email`, `can_release`, `reasons[]`, and flattened per extension segment a `BranchRight[]`
(`{branch, push, force, reason}`; branch is a name or a pattern such as `feature/*`).

**`AnalysisCreate`**: `kind` (`release` | `patch`); optional `fix_version`, `source` (default `[release].from`), `compare` (release: default `[release].to`; patch:
the release branch, required), `keep_empty` (default false; keep tickets without a DROP fix version).

**`AnalysisView`**: `id`, `created` (ms), `branch` (proposed for a release, the patched branch for a patch), `analysis` (`vdp_release::Analysis`: `request`, `merge_base`, `source_head`,
`compare_head`, `tickets`, `uncoupled`, `compare_reverts`, `dev_violations`, `release_only`, `keep_empty`), `selection[]` (tickets to revert for a release, to add for a patch),
`stale` (source or compare moved), `version`. **`AnalysisSummary`**: `id`, `created`, `kind`, `fix_version`, `branch`, `tickets`, `selected`. **`SelectionUpdate`**: `selection[]`, optional `version` (the
board's version; a different one on the server is a 409).

**`ReleaseCreate`**: `analysis_id`; optional `branch` (default the analysis' branch), `dry_run`, `force` (recreate: push with a lease on what origin has), `expect` (`OriginState {branch, head}` from the
preview's `outcome.origin`: refuse if origin moved), `accept_losses` (the preview's `losses` were shown and accepted), `on_conflict` (`pause` default | `theirs`), `strategy` (`theirs_hunks` default | `strict`).
**`PatchCreate`**: `analysis_id`; optional `dry_run`, `on_conflict`, `strategy`. **`PromotionCreate`**: `branch`; optional `kind` (`auto` default | `new` | `patch`), `dry_run`, `expect_main` (the preview's
`main_before`), `accept_rollback`, `accept_losses`. **`AnnouncementSend`**: `analysis_id` and `recipients[]` (both required: a body without `recipients` is a 400 `missing field`), optional `branch`. **`AnnouncementSent`**: `delivery` (`{mode: "sent", recipients}` |
`{mode: "preview_only", reason}`) and `announcement` (`{subject, html, text, included[], excluded[]}`; a row is `{id, url, summary, fix_version, status, committed_by, reporter}`).

**`Job`**: `id` (`j<n>`), `kind` (`clone`, `sync`, `analysis`, `release`, `patch`, `promotion`, `repair`, or an extension's own string), `title`, `status`, `started` and `finished` (ms),
`steps[]` (`{seq, ts, level: info|warn|error, message}`), `last_seq`, `result`, `error`, `pending` (`ConflictReport` while `waiting`). `status`: `running`, `waiting` (a conflict awaits a decision),
`interrupted` (it was running when the hub restarted), `done`, `failed`. `result` is tagged by `type`: `repo` (`repo: RepoStatus`), `analysis` (`analysis_id`), `release` and `patch` (`outcome: Outcome`),
`promotion` (`promoted: Promoted`), or an extension's document with its own `type`.

**`Outcome`** (release, patch): `branch`, `dry_run`, `origin` (`OriginState`; pass it back as `expect`), `losses[]`, `changelog`, `steps[]`, `validated`, `pushed`. **`Promoted`**: `branch`, `kind`, `dry_run`, `reason`,
`nothing_to_promote`, `rollback_from`, `main_before` (pass it back as `expect_main`), `merge_base`, `losses[]`, `main_head`, `pushed`.

**`ConflictReport`**: `title`, `step`, `operation` (`revert` | `cherry-pick` | `merge`), `files[]` (`{path, kind, touched_by[]}`; kind `content`, `change_deletes`, `branch_deleted`, `both_added`, `other`), `suggestions[]`
(`{ticket, action, reason}`). **`Resolution`**: `choices` (`path` to `change` | `branch` | `delete`), `action` (`continue` | `skip_step` | `abort`). **`ConflictVersionsView`**: `path`, `base`, `branch`, `change`
(the file's contents, null when absent), `binary`, `truncated`.

Other types (`Analysis`, `TicketEntry`, `ReleaseBranch`, `Detection`, `FileChanges`, `FileDiff`, ...) are defined in `vdp-release` and `vdp-notify` (the list is `typescript_with` in `ts.rs`).

## Errors

`{"code": "<code>", "error": "<message>"}`, built by `Reply::error(&HubError)`. `HubError` (`error.rs`):

| Variant | Status | `code` | Message |
|---|---|---|---|
| `BadRequest` | 400 | `bad_request` | as given |
| `Unauthorized` | 401 | `unauthorized` | as given |
| `Forbidden` | 403 | `forbidden` | as given |
| `NotFound` | 404 | `not_found` | `not found: <what>` |
| `Conflict` | 409 | `conflict` | as given |
| `Busy` | 409 | `busy` | `busy: <job id> is still running (<title>)` |
| `Internal` | 500 | `internal` | as given; a 5xx is also logged at error level |

Conversions: `ReleaseError::Invalid` is a 400; `ReleaseError::Stale` a 409 `stale analysis: ...`; every other `ReleaseError` and every `GitError` a 500 whose message is the cause chain
(`vdp_core::display_chain`). `NotifyError::NoRecipients` and `Address` are 400, other mail errors 500.

## Sign-in and rights

The rules, from `auth.rs` and `hub/host.rs`:

1. `POST /session {token}` trims the token and calls GitLab with it: `GET /user`, `GET /projects/<url-encoded path>` (access level = the larger of project and group access) and
   `GET /projects/<id>/protected_branches?per_page=100&page=<n>`, page after page until a page has fewer than 100 entries (at most 50 pages: a longer list is a 500).
2. Rights are evaluated for `main` (the local name of `[release].to`) and `release/*` (sample `<branch_prefix>2099_DROP99`). An unprotected branch takes push and force push from Developer (30) up.
   A protected one matches by name, else by `*` pattern; the person may push when a push level (or a named user) allows it, and force-push only when the rule has `allow_force_push`.
3. `can_release` is true when force push is allowed on both and the account has a commit e-mail (`commit_email`, else `email`, else `public_email`); otherwise `reasons` lists why. Not being allowed is
   not an error: the session is still created, so the person can read.
4. Each registered extension adds its own `BranchRight[]` to the user (`HubExtension::rights`), keyed by its segment.
5. The session (the person and the token) is kept in memory, 12 hours from creation, never renewed. Expired sessions are purged when a new session is created.
6. Jobs run as the person: commits carry their name and e-mail, pushes go to `push_url` with their token (git environment header `Authorization: Basic base64("oauth2:<token>")`); GitLab therefore enforces its
   own rules on the push. Clone, fetch and sync use the hub's own git credentials. With `auth = "none"`, commits use the clone's `user.name`/`user.email` and pushes go to `origin`.

## Extension routes

`ExtensionRequest.segments` is the path below the segment, already decoded and split; `query` is the raw query string. An extension answers with a `Reply` (`Reply::json(status, &value)`, or `Reply::error(&HubError)` for the shared error shape).
The `host` offers `actor(session, &ActorRequest)` and `pipeline(session, sha, project)`. See the README, "Extensions", and `tests/extension.rs`.
