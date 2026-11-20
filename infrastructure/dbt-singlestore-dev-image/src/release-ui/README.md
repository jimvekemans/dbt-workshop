# release-ui

The backend of the **release UI**: a Rust workspace that serves the HTTP API a web application uses to analyse, cut,
patch and promote dbt DROP releases (`release/<DROP>` branches, `INTEL-n` tickets, `changelog/<DROP>.md`), with GitLab
sign-in, Jira lookups and a release announcement by mail. The Svelte frontend is a separate repository and is not here.

**Not part of the workshop stack.** No compose service, Makefile target, nginx route, overview entry, CI job or manager
step uses it, it is built into no image, and `dbt-singlestore-workshop/vdp.toml` has no `[release_hub]` or `[notify]`
section. It is also unrelated to the workshop's Airflow `dbt_release` DAG and its tag-based release flow
(`.agents/skills/dbt-release-flow/SKILL.md`): the hub drives git, Jira and GitLab, not Airflow or SingleStore. It lives
here because `vaultspeed-framework` builds on it and because it is client code kept next to `vdp-rust-utils`.
[`.agents/skills/vdp-release-ui/references/deployment.md`](.agents/skills/vdp-release-ui/references/deployment.md) is a plan for wiring it into the stack,
not something that exists.

```
vdp-rust-utils  <--  release-ui  <--  vaultspeed-framework      (path dependencies, side by side in src/)
```

- `vdp-rust-utils`: the `vdp` toolkit (`vdp-cli`, `vdp-release`, `vdp-git`, `vdp-jira`, `vdp-secrets`, ...).
- `release-ui` (this workspace): `vdp-release-hub`, `vdp-notify`, `vdp-release-ui-cli`.
- `vaultspeed-framework`: VaultSpeed, its hub extension and the `vdp-vaultspeed` binary.

One binary, `vdp-release-ui`: the whole `vdp` command line plus one command of its own, `release serve`, which starts the
**release hub**. `vdp release-ui ...` hands over to it. Working on the code: [`CLAUDE.md`](CLAUDE.md) and the skill
[`.agents/skills/vdp-release-ui/SKILL.md`](.agents/skills/vdp-release-ui/SKILL.md).

## Build, install, run

```
cargo build --release --locked -p vdp-release-ui-cli      # target/release/vdp-release-ui
```

- Host `cargo` with `rustup`: `rust-toolchain.toml` pins channel 1.96 (rustfmt, clippy), edition 2024. The release profile is
  thin LTO, one codegen unit, stripped.
- The workspace imports `vdp-core`, `vdp-secrets`, `vdp-dbt`, `vdp-git`, `vdp-jira`, `vdp-release` and `vdp-cli` by path
  (`../vdp-rust-utils/crates/<name>`): `src/vdp-rust-utils` must sit next to `src/release-ui`. `Cargo.lock` is committed; use `--locked`.
- At run time the hub needs `git` on `PATH`. Jira, GitLab and SMTP are only needed when configured; the dbt engine only with `--validate`.
- Install `vdp-release-ui` next to `vdp` or on `PATH`. `vdp release-ui <args>` then replaces the `vdp` process (exec) with
  `vdp-release-ui <args>`, dropping the word `release-ui` and keeping global options as written
  (`vdp --env prd release-ui release serve`). Without the binary, `vdp` says so and exits 2. The dev image builds only
  `vdp` (`-p vdp-cli`); this binary is built by hand.

```
vdp-release-ui release serve                                       # all settings from vdp.toml
vdp-release-ui release serve --repo-dir /opt/project --port 8765 --ui-dir ../release-ui-frontend/build
vdp-release-ui release serve --cors-origin http://localhost:5173   # a frontend run with `npm run dev`
vdp-release-ui release serve --validate                            # run [release].validate (dbt) before a push
vdp release-ui release serve                                       # the same through the vdp binary
```

Once it listens, `release serve` prints one line on stderr (also under `-q`) and runs until it is killed:

```
release hub on http://<address>/api/v1 (repository <repo_dir>[, UI from <ui_dir>])
```

`--port 0` picks a free port; the line says which. Nothing is cloned, fetched or written at start: the clone is created by
a `POST /repo/clone` job (the clone action of the UI). There is no signal handling: Ctrl-C or SIGTERM ends the process at
once and a running job is cut off (see "Jobs" below for what the next start shows).

**Serving the frontend.** `--ui-dir <dir>` (else `[release_hub].ui_dir`) names the built frontend. The hub then answers
`GET`/`HEAD` outside `/api/v1` from that directory and falls back to `index.html` for paths without a file extension (a
single-page app). The directory must contain `index.html`, or the command exits with 2. Without it the hub is API only and
every path outside `/api/v1` is a 404. The frontend is not served by nginx in this repository (nginx here serves only the
dbt docs sites).

**Try it without GitLab or Jira.** A throwaway origin with `main` and a `dev` that has three ticket merges (the default
`ticket_prefix` is `INTEL`; git needs a configured identity):

```
git init -q --bare origin.git && git clone -q origin.git seed && cd seed
git checkout -q -b main && echo base > README.md && git add . && git commit -qm initial && git push -q origin main
git checkout -q -b dev
for n in 1 2 3; do git checkout -q -b feature/t$n dev; echo $n > t$n.sql; git add .; git commit -qm "INTEL-$n add t$n"
  git checkout -q dev; git merge -q --no-ff -m "Merge INTEL-$n add t$n" feature/t$n; done
git push -q origin dev; cd ..
printf '[release_hub]\nauth = "none"\nrepo_url = "%s/origin.git"\nstate_dir = "%s/state"\n' $PWD $PWD > hub.toml
vdp-release-ui --no-vault --config hub.toml release serve --bind 127.0.0.1 --port 8765 --repo-dir $PWD/clone
```

On stderr, after two warnings (no Jira connection, sign-in off): `release hub on http://127.0.0.1:8765/api/v1 (repository <dir>/clone)`.
From another shell:

```
curl -s -X POST localhost:8765/api/v1/repo/clone                 # a job: poll GET /api/v1/jobs/j1
curl -s -X PUT localhost:8765/api/v1/identity -d '{"name":"Test","email":"test@example.com"}'
curl -s -X POST localhost:8765/api/v1/analyses -d '{"kind":"release","fix_version":"2026_DROP05"}'   # job j2
curl -s localhost:8765/api/v1/analyses
[{"id":"a1",...,"kind":"release","fix_version":"2026_DROP05","branch":"release/2026_DROP05","tickets":3,"selected":3}]
curl -s localhost:8765/api/v1/status                             # excerpt:
..."auth":"none","jira":{"ready":false,"detail":"jira: secret JIRA_USER not found in dotenv (none), process environment"},
"smtp":{"ready":false,"detail":"SMTP credentials not configured (SMTP_USER / SMTP_PASSWORD)"}...
```

All three tickets are selected by default (for a release the selection is what gets reverted): without Jira data no ticket has a
DROP fix version (`classification: no_fix_version`). An announcement would only be previewed, as no SMTP account is set.

## Commands, options, exit codes

`vdp-release-ui` accepts every command of `vdp` with the same options and help (`vdp-release-ui <command> --help`):
`dbt`, `sources` (`add`, `rewrite`), `release` (`create`, `analyse`, `patch`, `promote`, `branches`), `jira` (`advance`),
`sql` (`run`, `export`), `qa` (`orphan-tables`, `view-candidates`), `env` (`show`, `doctor`), `dev` (`grammar`). They are
documented with `vdp-rust-utils`. What this binary adds is `release serve`. `vdp`'s own hand-offs (`vaultspeed ...`) are
not commands here: `vdp-release-ui vaultspeed plan` is an unrecognised subcommand, exit code 2.

`release serve`: every option defaults to its `[release_hub]` setting; an option wins over the setting.

| Option | Overrides | Default | Notes |
|---|---|---|---|
| `--repo-dir <DIR>` | `repo_dir` | `/opt/project` | relative paths are taken from the current directory |
| `--bind <ADDRESS>` | `bind` | `0.0.0.0` | `127.0.0.1` for local use; an IPv6 address works (`::1`) |
| `--port <PORT>` | `port` | `8765` | `0` picks a free port |
| `--ui-dir <DIR>` | `ui_dir` | empty: API only | needs `index.html`; relative paths from the current directory |
| `--cors-origin <ORIGIN>` | `cors_origin` | empty: same origin only | one origin |
| `--validate` | `validate` | off | runs `[release].validate` in the clone before a push; needs the dbt engine (`env doctor`); the flag can only switch it on |

Global options (before or after the command; all are shared with `vdp`):

| Option | Env var | Meaning |
|---|---|---|
| `--env <ENV>` | `DBT_DEPLOY_TARGET` | environment: `LDV`, `ABT`, `CBT`, `PRD` or an `[environments.<name>]`; `dev`, `val`, `prd` are aliases of LDV, ABT, PRD; else `[core].default_environment`; unknown name: exit 2 |
| `--project-dir <DIR>` | `VDP_PROJECT_DIR` | dbt project (has `dbt_project.yml`); default the nearest parent of the current directory; optional for `release serve` |
| `--config <FILE>` | `VDP_CONFIG` | extra TOML file layered over the other configuration files; must exist |
| `--target <TARGET>` | | dbt target the toolkit itself uses |
| `--profiles-dir <DIR>` | `DBT_PROFILES_DIR` (search order, see `--help`) | where `profiles.yml` is |
| `--no-vault` | `VDP_NO_VAULT` (`1`/`true`/`yes`/`on`) | do not log in to Vault; secrets from the dotenv file and the environment only |
| `-v`, `--verbose` | `VDP_LOG` (a tracing filter, overrides `-v`/`-q`) | debug logging on stderr |
| `-q`, `--quiet` | | warnings and errors only (the "release hub on ..." line is still printed) |
| `--log-json <FILE>` | `VDP_LOG_JSON` | also write the log as NDJSON (info and above, debug with `-v`) |
| `-h`, `-V` | | help, version |

Other variables: `VDP_TS_OUT=<file>` (TypeScript bindings, see "Extensions"). Secrets named in `[jira]` and `[notify]`
(`JIRA_USER`, `JIRA_TOKEN`, `SMTP_USER`, `SMTP_PASSWORD`, `SMTP_FROM` by default) may come from the process environment.

Exit codes: `0` success; `1` the command ran but some of its work failed; `2` setup or usage error (bad arguments, invalid
or unreadable configuration, unknown environment, a refused operation). `release serve` runs until killed and exits with 2
when it cannot start: invalid configuration (for example `auth = "ldap"`), `--ui-dir` without `index.html`, secrets that cannot be resolved,
an address it cannot listen on.

## Configuration

The same `vdp.toml` as `vdp`, layered later-wins key by key: built-in defaults, `/etc/vdp/vdp.toml`, the dbt project's
`vdp.toml`, then `--config FILE`. `release serve` reads `[release_hub]`, `[notify]`, `[release]` and `[jira]`, plus the
shared `[core]` and `[secrets]` (and `[dbt]`, for `--validate`). `vdp` itself knows `[release_hub]` and `[notify]` as module
sections and does not warn about them. `[git]` is also accepted as a section name but has no settings and nothing reads it.

**`[release_hub]`** (`HubSettings`; no `deny_unknown_fields`):

| Key | Default | Meaning |
|---|---|---|
| `repo_dir` | `/opt/project` | the clone the hub analyses and releases from (created and synced by jobs) |
| `repo_url` | `git@git.example.com:vdp/dbt-vdp.git` | what the clone action proposes; also where GitLab settings are derived from |
| `bind` | `0.0.0.0` | listen address |
| `port` | `8765` | listen port |
| `cors_origin` | `""` | `Access-Control-Allow-Origin` for a frontend on another origin; empty: same origin only |
| `ui_dir` | `""` | built frontend to serve next to the API; empty: API only |
| `validate` | `false` | run `[release].validate` (dbt) in the clone before a release is pushed |
| `keep_jobs` | `50` | jobs kept (finished ones are dropped oldest first; at least 1) |
| `git_timeout_secs` | `600` | a git command running longer is killed; `0`: no limit |
| `conflict_timeout_secs` | `7200` | a conflict without a decision for this long aborts the run (nothing is pushed). Read from the code: `0` aborts at the first conflict |
| `state_dir` | `/var/lib/vdp-hub` | `analyses.json` and `jobs.json` survive restarts here; empty: memory only |
| `auth` | `gitlab` | `gitlab`: sign in with a personal GitLab token, release rights from GitLab; `none`: no sign-in (local use); anything else: exit 2 |
| `gitlab_url` | `""` | GitLab address; empty: derived from `repo_url` |
| `gitlab_project` | `""` | project path (`group/project`); empty: derived from `repo_url` |
| `push_url` | `""` | where pushes go; empty: `https://<host>/<project>.git` |
| `[release_hub.<name>]` | | the settings of the extension served at `/api/v1/<name>/`, read by that extension |

GitLab derivation (`GitLab::from_repo_url`): `git@host:group/project.git`, `ssh://git@host[:port]/group/project.git` and
`https://host/group/project[.git]` give `https://host`, `group/project` and `https://host/group/project.git`; a path on disk gives
nothing. It always derives `https`, so a GitLab on plain `http` needs all three of `gitlab_url`, `gitlab_project` and
`push_url` set explicitly (an `http://` `repo_url` is not among the tested inputs; read from the code, it derives a wrong host).
A key that is not a `HubSettings` field is not an error: it is taken for an extension table, and the start warns
`[release_hub.<key>] is set, but this binary serves no such release extension`. Wrong value types are errors.

**`[notify]`** (`NotifySettings`; `deny_unknown_fields`, so a typo is an error):

| Key | Default | Meaning |
|---|---|---|
| `smtp_host` | `smtp.gmail.com` | SMTP server |
| `smtp_port` | `587` | |
| `security` | `starttls` | `starttls` (587), `tls` (465), `none` (unencrypted, for a local relay or a test server) |
| `from` | `""` | sender; empty: the secret named by `from_secret`, else the SMTP user |
| `user_secret` | `SMTP_USER` | secret holding the SMTP account |
| `password_secret` | `SMTP_PASSWORD` | secret holding its password |
| `from_secret` | `SMTP_FROM` | secret holding the sender |
| `default_recipients` | `[]` | recipients the UI proposes (`defaults.recipients` in the status) |
| `timeout_secs` | `15` | SMTP connection timeout |

The mailer is ready when `smtp_host` is set, a sender is known, and (unless `security = "none"`) user and password are set.
Not ready is not an error: `POST /announcements` then answers `delivery: {"mode": "preview_only", "reason": ...}` and
sends nothing; the reason is also in `GET /status` under `smtp`.

**Sections of the release flows** the hub shares with `vdp release ...` (both `deny_unknown_fields`):

| Key | Default | Meaning |
|---|---|---|
| `[release].ticket_prefix` | `INTEL` | Jira project key in commit messages |
| `[release].from` | `origin/dev` | source of releases; its local name (`dev`) is never written |
| `[release].to` | `origin/main` | where releases go; written by promotions only |
| `[release].branch_prefix` | `release/` | prefix of release branches |
| `[release].validate` | `["compile", "--no-partial-parse"]` | dbt arguments run in the clone before a push (with `--validate`) |
| `[jira].base_url` | `https://issuetracker.example.com` | Jira |
| `[jira].user_secret` / `token_secret` | `JIRA_USER` / `JIRA_TOKEN` | secrets with the Jira account and API token |
| `[jira].excluded_fix_versions` | `["Nog in te plannen"]` | fix versions that take a ticket out of a release |
| `[jira].sprint_field` | `customfield_11221` | custom field with the sprints |
| `[jira].advance_transition` | `""` | used by `vdp jira advance` only |

Secrets come from the sources and order of `[secrets].precedence` (Vault paths, the dotenv file `[core].dotenv`, the
process environment; `--no-vault` skips Vault). A missing Jira connection is a warning at start, not an error.
The same tables with more detail are in [`.agents/skills/vdp-release-ui/references/config.md`](.agents/skills/vdp-release-ui/references/config.md).

## The HTTP API

Everything is JSON under `/api/v1` (`http::API`); a leading `/dbt/` is accepted and ignored (`/api/v1/dbt/status` is
`/api/v1/status`). Paths are percent-decoded before routing; query strings decode `+` as a space. A trailing slash is
ignored in routes, but `/api/v1/` itself is a 404. API replies carry `Cache-Control: no-store`.

**Rights.** With `auth = "gitlab"`, every route except `GET /status` and `/session` needs a signed-in session (401
otherwise); routes marked *rights* also need `can_release` (403 otherwise). With `auth = "none"` nothing is checked and git
commits use the clone's identity (`PUT /identity`). Unknown routes are 404 (`not found: GET /api/v1/nope`).

| Method and path | Needs | Body or query | Answer |
|---|---|---|---|
| `GET /status` | public | | `Status` |
| `POST /session` | public | `{"token"}` (`SignIn`) | `SessionUser`, sets the cookie; 400 when `auth = "none"` |
| `GET /session` | public | | `SessionUser`, or 401 `not signed in` |
| `DELETE /session` | public | | 204, cookie cleared |
| `PUT /identity` | sign-in | `GitIdentity` | `GitIdentity`; 400 when `auth = "gitlab"` (commits use the GitLab account) |
| `POST /repo/clone` | rights | `{"url"?}` (`CloneRequest`) | `Job` (202) |
| `POST /repo/sync` | sign-in | | `Job` (202): fetch all branches and tags, pruned |
| `POST /repo/repair` | rights | | `Job` (202): abort what is in progress, remove a stale lock, reset, detach |
| `POST /repo/reclone` | rights | `{"url"?}` | `Job` (202): delete the clone, clone again |
| `GET /release-branches` | sign-in | | `ReleaseBranch[]` (newest first) |
| `GET /release-branches/<branch>/promotion` | sign-in | branch has slashes: `/release-branches/release/2026_DROP05/promotion` | `Detection` |
| `GET /changes` | sign-in | `?from=&to=` (commit id or `origin/<branch>`) | `FileChanges` |
| `GET /changes/promotion` | sign-in | `?branch=` | `FileChanges` |
| `GET /changes/file` | sign-in | `?from=&to=&path=` | `FileDiff` (cut at 256 KiB) |
| `GET /analyses` | sign-in | | `AnalysisSummary[]` (newest first) |
| `POST /analyses` | sign-in | `AnalysisCreate` | `Job` (202); result `analysis_id` |
| `GET /analyses/<id>` | sign-in | | `AnalysisView` |
| `PUT /analyses/<id>/selection` | sign-in | `SelectionUpdate` | `AnalysisView`; 409 when `version` is old |
| `GET /analyses/<id>/announcement` | sign-in | `?branch=` | `Announcement` |
| `POST /releases` | rights | `ReleaseCreate` | `Job` (202) |
| `POST /patches` | rights | `PatchCreate` | `Job` (202) |
| `POST /promotions` | rights | `PromotionCreate` | `Job` (202) |
| `POST /announcements` | sign-in | `AnnouncementSend` | `AnnouncementSent` (200) |
| `GET /jobs` | sign-in | | `Job[]` without steps, newest first |
| `GET /jobs/<id>` | sign-in | `?after=<seq>` | `Job` with the steps after `seq` |
| `POST /jobs/<id>/resolution` | rights | `Resolution` | the `Job` (200) |
| `GET /jobs/<id>/conflict` | sign-in | `?path=` | `ConflictVersionsView` |
| `<METHOD> /<segment>/...` | sign-in | | whatever the extension answers |

Request bodies refuse unknown fields (400 naming the field), except `GitIdentity` (`PUT /identity`) and `Resolution` (`POST /jobs/<id>/resolution`), which ignore them; an empty body counts as `{}`; a body over 1 MiB is a 400
`request body too large`. Job ids are `j<n>`, analysis ids `a<n>`. The types are in `crates/vdp-release-hub/src/api.rs` (bodies, status, analyses), `jobs.rs` (`Job`, `JobKind`, `JobStatus`,
`JobStep`, `Level`, `JobResult`) and `auth.rs` (`SessionUser`, `BranchRight`, `Pipeline`), or in `vdp-release` and `vdp-notify`
(the flows' types, `Announcement`); with the `ts` feature they are all in the generated TypeScript. Field-level detail:
[`.agents/skills/vdp-release-ui/references/api.md`](.agents/skills/vdp-release-ui/references/api.md).

**Errors** are `{"code": ..., "error": "<message>"}`:

| Status | `code` | Raised when |
|---|---|---|
| 400 | `bad_request` | invalid body or parameter, a refused ref, no recipients, sign-in is off |
| 401 | `unauthorized` | no or expired session, GitLab refused the token |
| 403 | `forbidden` | no release rights, a token without access to the project, no commit e-mail |
| 404 | `not_found` | unknown route, analysis, job, or branch on origin |
| 409 | `conflict` | no clone yet, clone exists, a stale analysis or board version, no git identity, a job is not waiting |
| 409 | `busy` | another job is running |
| 500 | `internal` | git failed, GitLab or SMTP unreachable, the job thread could not start |

Both 409 answers differ only in `code`: a client tells "another job is running, retry later" (`busy`) from "the request does not fit the state" (`conflict`) by that field.

**Sessions.** `POST /session` with a personal GitLab token (scopes `read_api`, `write_repository`) asks GitLab
(`{gitlab_url}/api/v4`, header `PRIVATE-TOKEN`) who the token belongs to and what that person may push to `main` and
`release/*` (project and group access level and the protected-branch rules). `can_release` is true when force push is
allowed on both and the account has a commit e-mail; the person's `reasons` say why not. The answer carries
`Set-Cookie: vdp_session=<48 hex>; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200`.

**The token is held in the server's memory for 12 hours** (`SESSION_HOURS`, absolute, not renewed; a restart signs everyone
out). Commits are made as that person, and pushes go to `push_url` with the token as an `Authorization` header in the git
child process's environment (never in its arguments or on disk), so GitLab enforces its own rules. Jira and SMTP credentials
never reach the browser; the only secret the browser sends is the person's own GitLab token, once, in this POST.

**CORS** headers (`Allow-Origin` with the configured origin, methods `GET, POST, PUT, DELETE, OPTIONS`, header
`Content-Type`, credentials `true`, `Vary: Origin`) are sent only when `--cors-origin` or `cors_origin` is set. Every `OPTIONS`
below `/api/v1` is answered 204 without further checks. **Limits:** one request per thread; request bodies 1 MiB; diffs
and conflict versions 256 KiB; GitLab calls time out after 20 s; there is no TLS (plain HTTP).

**Jobs.** Git work runs as a job on its own thread; the hub runs one at a time (`409 busy`, naming the running job). A job has
a `status` (`running`, `waiting`, `interrupted`, `done`, `failed`), a step log read by cursor (`GET /jobs/<id>?after=<seq>`:
poll with the `last_seq` of the previous answer), and a typed `result` (`repo`, `analysis`, `release`, `patch`, `promotion`,
or an extension's own document) or an `error`. After a job the clone goes back to a detached HEAD at the source branch on
origin (`origin/dev`) without local branches. If it is dirty or an operation is left in progress, the job says so and
`GET /status` lists `repo.health` problems (`in_progress`, `index_lock`, `dirty`, `local_branches`, `broken`) until
`POST /repo/repair` or `/repo/reclone` fixes them. With `state_dir` set, `analyses.json` and `jobs.json` are written
atomically (temp file, rename); after a restart a job that was running or waiting is `interrupted`.

**The conflict pause.** When applying a ticket (revert, re-enable, cherry-pick) conflicts and `on_conflict` is `pause` (the
default of releases and patches; `theirs` resolves for the change being applied; `strategy` is `theirs_hunks` or `strict`),
the job becomes `waiting` and its `pending` holds a `ConflictReport` (the files, the tickets that touched them, suggested
selection changes). The UI shows each file's versions with `GET /jobs/<id>/conflict?path=<file>` (`base`, `branch`,
`change`) and answers with `POST /jobs/<id>/resolution`:

```
{"choices": {"<path>": "change" | "branch" | "delete"}, "action": "continue" | "skip_step" | "abort"}
```

The job resumes (`running`). A path that is not in the conflict is a 400; a job that is not waiting is a 409; no decision
within `conflict_timeout_secs` aborts the run and nothing is pushed. Answering needs release rights and a git identity.
A promotion takes neither option and gets the same decision channel; whether and when it pauses is decided in
`vdp-release` (not verified here).

**Preview, then execute.** An analysis stores the heads it saw. `POST /releases`, `/patches` and `/promotions` refuse a
stale analysis (409 `stale analysis`), take `dry_run` for a preview, and bind the execution to what the preview showed:
`expect` (the branch head on origin) and `accept_losses` for releases, `expect_main`, `accept_rollback` and `accept_losses` for
promotions. Only the newest two release branches can be released, patched or promoted (the hub has no override), and
`dev` is never written.

## Extensions

A second repository can be steered from the same UI below its own URL segment, `/api/v1/<segment>/...`. The hub keeps what
must stay in one place (sessions, GitLab sign-in, the error shape, `GET /status`); the extension keeps its own clone, jobs
and settings. `vdp-vaultspeed` (in `vaultspeed-framework`) is the one real extension, segment `vaultspeed`.

The interface (`vdp_release_hub::extension`):

| Item | Role |
|---|---|
| `HubExtension` (trait, `Send + Sync + Debug`) | `segment()`: the URL segment and the key in the status and in `SessionUser`; `summary()`: JSON for `GET /status` under that key; `rights(&GitLab, token)`: the person's `BranchRight`s on the extension's branches, asked once at sign-in; `handle(&ExtensionRequest)`: answers one request |
| `ExtensionRequest` | `method`, `segments` (decoded path below the segment), `query`, `body`, `session`, `host`; helpers `param`, `param_opt`, `json::<T>()`, `actor(&ActorRequest)` |
| `ExtensionHost` | what the hub offers: `actor(session, &ActorRequest)` (who a write acts as: the signed-in person who holds the right, or the clone identity without sign-in) and `pipeline(...)` (newest CI pipeline for a sha) |
| `ActorRequest` | `branch`, `rights_branch` (the rule that covers it, e.g. `feature/*`), `need: Need`, `push_to: Option<&GitLab>` |
| `Need` | `Push` or `Force` (resets and recreated releases) |

To write and mount one:

1. Implement `HubExtension` in your crate (depend on `vdp-release-hub`, not on `vdp-release-ui-cli`). Use
   `CloneManager`/`CloneConfig`, `Jobs`/`JobKind::Other("<name>")`/`JobResult::Extension(value)` for its own clone and jobs; do writes
   through `request.actor(...)` so rights and push targets match the hub's.
2. Read its settings with `HubSettings::extension::<T>("<segment>")` (`T: DeserializeOwned + Default`; the table is `[release_hub.<segment>]`).
3. Write a factory `Fn(&Context, &ExtensionInputs) -> anyhow::Result<Vec<Arc<dyn HubExtension>>>` (`ExtensionInputs`: the
   `[release_hub]` settings, whether sign-in is on, the Jira tracker) and run the command line with
   `vdp_release_ui_cli::run_command(&global, command, &factory)`; `hub::no_extensions` is the factory of `vdp-release-ui`.
4. TypeScript: export your types with `vdp_release_hub::declaration::<T>()` and append them with `typescript_with(&decls)`.

Extension requests need a sign-in like every other route (checked before `handle`); the rights check belongs to the extension.
An extension is matched on the first path segment, after a leading `dbt` segment has been dropped and before the core routes: a segment
equal to a core first segment (`status`, `session`, `jobs`, `repo`, ...) shadows that core route, and an extension named `dbt` is
reachable only as `/api/v1/dbt/dbt/...` (read from `route`; not tested). `vdp-vaultspeed` keeps its state under `<state_dir>/vaultspeed`.
`tests/extension.rs` shows the smallest working extension (a fake that echoes the request).

**TypeScript bindings** for the frontend come from the Rust types through ts-rs (feature `ts`, chained through
`vdp-release/ts` and `vdp-notify/ts`):

```
VDP_TS_OUT=/tmp/api.ts cargo test -p vdp-release-hub --features ts export_bindings
```

writes the whole module of the hub's types (declaration count: see "Testing") to `$VDP_TS_OUT`. The list of exported types is
`typescript_with` in `crates/vdp-release-hub/src/ts.rs`: a new API type must be added there. With the VaultSpeed extension in use, the
frontend takes the **complete** file from `cargo test -p vdp-vaultspeed-hub --features ts export_bindings` in
`vaultspeed-framework`: that test calls `typescript_with` with VaultSpeed's declarations and writes the hub's types plus
VaultSpeed's to `$VDP_TS_OUT`.

## Crates and architecture

| Crate | Content | Entry |
|---|---|---|
| `vdp-release-hub` | the hub: clone, analyses, jobs, sign-in, HTTP API and static frontend, the extension interface | `src/lib.rs`, `src/http.rs` (`route`), `src/hub/mod.rs` (`Hub`) |
| `vdp-notify` | the announcement (subject, HTML, text) and its delivery by SMTP (`Mailer`, `SmtpMailer`, lettre) | `src/lib.rs` |
| `vdp-release-ui-cli` | the composition root and the `vdp-release-ui` binary; as a library `args`, `hub::serve`, `hub::ExtensionFactory`, `run_command` | `src/main.rs`, `src/hub.rs` |

`vdp-release-hub` modules: `api` (request and response bodies), `auth` (GitLab client, `Sessions`, `BranchRight`), `clone_manager`
(clone, sync, repair, health; reused by extensions), `error` (`HubError` to status and code), `extension`, `http`
(`route`, `HubServer`), `hub` (`Hub` with `analyses`, `releases`, `announcements`, `host`), `jobs` (`Jobs`, `JobLog`, conflict wait),
`settings` (`HubSettings`), `ts`. The domain logic (analysis, the release, patch and promotion flows, conflicts, the changelog, the
git and Jira adapters) is **not here**: it is `vdp-release`, `vdp-git` and `vdp-jira` in `vdp-rust-utils`, shared with `vdp release ...`, so
the CLI and the UI decide the same way.

```
browser --HTTP--> http::handle (thread per request, tiny_http) --> http::route (pure: method, path, query, body, session -> Reply)
                                                                         |-- extension.handle(...)           (first segment matches)
                                                                         '-- Hub methods --> Jobs (one at a time, own thread)
                                                                                              '--> vdp-release flows over GitCli (WriteGuard), Jira, Mailer
```

`vdp-release-ui-cli` embeds `vdp-cli` as a library. `args::Cli` is `vdp`'s `GlobalArgs` plus `args::Command`, which is `ReleaseServe`
or `Core(vdp's command)`. It is merged into clap by hand instead of `#[command(flatten)]`: `vdp`'s command enum ends in an external
subcommand, and a flattened enum would route every subcommand name into it. `main` rejects that external fallback
(`vdp`'s hand-offs are not commands here) and runs everything else through `run_command`; `hub::serve` wires the hub to the
services of `vdp_cli::bootstrap::context` (Jira, the secret chain, the dbt engine for `--validate`, SMTP). There is no async
runtime: `tiny_http` serves, `ureq` calls GitLab, `lettre` sends the mail.

Decisions the code encodes (user decisions recorded with the original code; confirm before changing):

- Conflicts pause the job and the release manager decides per file.
- `dev` is never written, not even locally: `BranchRules` validates targets, `GitCli`'s `WriteGuard` allows only `release/*` and `main`, and the
  hub's clone keeps no local branches. `main` changes only by promotion.
- Only people who may force-push `main` and `release/*` in GitLab can release; they push as themselves.
- Every release branch ends with `changelog/<DROP>.md`, rewritten at the tip by a patch (the hub always writes it).
- Only the newest two release branches can change; the hub passes no `--allow-old-release`.
- Commits that only `main` or a release branch has (`release_only` in the analysis) reach `dev` through the developers' own merge request; a
  new promotion that would drop them needs `accept_losses`.
- A promotion pushes `main` with a lease bound to the head the preview saw (`expect_main`).

## Differences from the Python release UI

| Python release UI | This backend | Where |
|---|---|---|
| Jira and SMTP credentials partly sent by the browser | only on the server (secret chain); the API reports readiness (`jira`, `smtp` in `GET /status`) | `vdp-release-hub` `api.rs` |
| SMTP server fixed (`smtp.gmail.com:587`, STARTTLS) | `[notify]`: host, port and `security`, with those defaults | `vdp-notify` `settings.rs` |

The flows themselves are those of `vdp release ...`; their differences from the Python tools are described with `vdp-rust-utils`.

## Pointing it at the workshop

**Untested recipe.** The workshop stack has a GitLab with the project `workshop/dbt-vdp` (default branch `dev`, protected `dev`
and `main`, force push allowed on `main`, created public by `bootstrap_stack.py`) and a Mailpit. Nothing in the stack runs the hub; this
runs it next to the stack:

```toml
# hub.toml
[release_hub]
repo_url       = "http://gitlab.local/workshop/dbt-vdp.git"
gitlab_url     = "http://gitlab.local"            # explicit, because repo_url is http: the derivation always yields https
gitlab_project = "workshop/dbt-vdp"
push_url       = "http://gitlab.local/workshop/dbt-vdp.git"
state_dir      = "/tmp/vdp-hub-state"

[notify]
smtp_host = "mailpit"
smtp_port = 1025
security  = "none"
from      = "release-ui@example.com"              # a sender is required even without credentials
```

```
vdp-release-ui --no-vault --config hub.toml release serve --bind 127.0.0.1 --port 8765 --repo-dir /tmp/vdp-hub-clone
```

- Nothing in the stack uses port 8765.
- Sign in with `POST /api/v1/session {"token": "<personal access token>"}` of a workshop maintainer (scopes `read_api`,
  `write_repository`); the account needs a commit e-mail. Only maintainers and above can push `main`.
- Clone and fetch run with the hub's own git credentials (here anonymous HTTPS to a public project); only pushes carry the signed-in person's token.
- Mailpit shows the announcement at `http://localhost:8025`. **From the host `mailpit` does not resolve** and compose publishes only
  Mailpit's web port (8025), not SMTP (1025): either run the hub on `workshop_net` (see `.agents/skills/vdp-release-ui/references/deployment.md`) or publish 1025 (a compose change, not made).
- The workshop's release flow is tag-based (`dbt-release-flow` skill), not DROP-based: its commits carry no `INTEL-n` tickets and there is no Jira,
  so analyses will list nothing useful. What this recipe can show is sign-in, the clone, repo health, `GET /changes`, and the announcement preview.

## Testing

`cargo test --workspace --locked` runs 38 tests and 4 doc tests (all pass; `--all-features` adds the `ts` export test: 39 and 4).
The TypeScript export writes 67 `export type` declarations. These counts are kept here only: update them when a route, type or test is added.
Gates that pass on 1.96: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`. The rule is also to run them with `cargo +stable` (not run in this round).

**In Docker, without a Rust toolchain on the host.** From the repository root:

```
make rust-check W=release-ui                    # fmt, clippy (all features), tests (default features), docs
make rust-test W=release-ui S="-p vdp-notify"   # only tests; S= goes to cargo test
make rust-check                                 # all three workspaces: after a change in vdp-rust-utils or in a public item here
```

The toolchain image is `workshop-rust-check` (`infrastructure/rust-check/`, `rust:1.96-trixie`, the version read from `ARG RUST_VERSION` of the dev image
`Dockerfile`); it runs as the host user and mounts the whole `src/` folder, because this workspace reaches `vdp-rust-utils` by path. The cargo
registry and the build output live in Docker volumes (`make rust-clean` removes them), not in `target/`. `CARGO_BUILD_JOBS=<n>` limits the
parallelism; `make rust-cargo W=release-ui S="<cargo arguments>"` runs any other cargo command. The TypeScript export writes a file you
choose (`VDP_TS_OUT`): run that one with a host `cargo`.

| Crate | Unit | Integration | Doc | What they cover |
|---|---|---|---|---|
| `vdp-notify` | 2 | | 1 | subject, HTML escaping; the preview-only delivery without an SMTP account (doc test) |
| `vdp-release-hub` | 17 (18 with `ts`) | `tests/clone_lifecycle.rs` 1, `tests/extension.rs` 4, `tests/gitlab_rights.rs` 1 | 2 | GitLab URL derivation, settings resolution and force-push rule evaluation; percent decoding, static files stay inside the root, secrets stay out of `Debug`; one job at a time with a cursor, failures and panics end a job, a job whose thread cannot start leaves no trace and the next one runs; protected branches are read page after page (the paging loop, and over HTTP against a stand-in GitLab: a rule on page 2 counts); commit identity injection; tidy; the clone against a local bare repository (identity, protected branch, repair, sync, re-clone); a fake extension: routing below its segment, status keyed by segment, rights and job results under their own names; TypeScript export |
| `vdp-release-ui-cli` | 8 | `tests/process.rs` 5 | 1 | the command tree (valid, every command has `about`, `long_about`, an `Examples:` block and help for every option, help lines within 100 columns, `serve` only below `release`), the `--ui-dir` help names no workshop web server; the binary as a process: the banner under `-q`, help, an unknown command is exit 2 |

Not covered here: GitLab sign-in beyond `GitLab::rights` (tested against a stand-in GitLab; not `GitLab::user`, `pipeline`, the 401 and 403 mapping or the HTTPS push path), Jira, SMTP
delivery (`SmtpMailer::send`), the release, patch and promotion jobs and the conflict pause end to end (`JobLog::wait_for`, `resolve`),
analysis and job persistence across a restart, the core routes through `route` (only `/status` and extension routing are tested), CORS headers, the body
limit, serving files over a socket, the announcement's full HTML, and the generated TypeScript beyond "contains `Analysis`, no `bigint`".
Routes and replies were also checked by hand against a throwaway hub on a local bare repository with `auth = "none"`; that is not an automated test.

## Known gaps

- The frontend is not in this repository; nothing here builds or tests it.
- Plain HTTP only, no TLS and no rate limiting: keep the hub on loopback or behind a TLS-terminating proxy. The session cookie has no `Secure` flag.
- `GET /status` is public and shows the clone directory, the clone's `origin` URL (do not put credentials in `repo_url`), the Jira and SMTP readiness
  reasons and the default recipients.
- The hub has one clone and one job at a time for everyone: a sync or an analysis blocks a release, and `409 busy` is the answer.
- No signal handling: stopping the process cuts a running job off mid-git; the next start marks it `interrupted` and `repo.health` shows what is left.
- The session expires after a fixed 12 hours.
- The release flows (`vdp-release`) come from a cleaned copy of `vdp-rust-utils` that has no `tests/` folder for them (about twenty unit tests at the time of writing), and this
  workspace has no whole-story test over HTTP. The release, patch and promotion paths therefore have no automated test in this copy.
