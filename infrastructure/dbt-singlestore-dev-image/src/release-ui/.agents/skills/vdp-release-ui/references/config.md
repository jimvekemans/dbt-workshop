# Configuration reference

Everything `vdp-release-ui release serve` reads. The README carries the same tables in shorter form; this file is the
place to change when a setting changes. Sources of truth: `HubSettings` (`crates/vdp-release-hub/src/settings.rs`),
`NotifySettings` (`crates/vdp-notify/src/settings.rs`), `ReleaseSettings` and `JiraSettings` (in `vdp-rust-utils`), and
`hub::serve` (`crates/vdp-release-ui-cli/src/hub.rs`), which reads them.

## Layers

One `vdp.toml` format for the whole `vdp` family. Later files win, key by key:

1. built-in defaults,
2. `/etc/vdp/vdp.toml`,
3. `vdp.toml` of the dbt project (`--project-dir` / `$VDP_PROJECT_DIR`, else the nearest parent of the current directory that
   has a `dbt_project.yml`; `release serve` also works with no project at all),
4. `--config FILE` / `$VDP_CONFIG` (must exist).

A `release serve` option then wins over its `[release_hub]` setting. `vdp` accepts `[release_hub]` and `[notify]` as module
sections (`MODULE_SECTIONS` in `vdp-cli`), so the same file can serve `vdp` and `vdp-release-ui`. `[git]` is in that list too but no
settings struct reads it.

| Section | Read by `release serve` | Strict (`deny_unknown_fields`) |
|---|---|---|
| `[release_hub]` | yes, `HubSettings` | no (see "Unknown keys") |
| `[release_hub.<name>]` | by the extension `<name>` | its own decision |
| `[notify]` | yes, `NotifySettings` | yes |
| `[release]` | yes, `ReleaseSettings` | yes |
| `[jira]` | yes, `JiraSettings` (`base_url`, `excluded_fix_versions` into the hub; account and `sprint_field` into the Jira client) | yes |
| `[core]`, `[secrets]`, `[environments.<name>]` | yes, by `vdp-core` (environment, secret chain) | see `vdp-rust-utils` |
| `[dbt]` | only with `--validate` (the dbt engine service) | see `vdp-rust-utils` |

## `[release_hub]`

| Key | Type | Default | `release serve` option | Effect |
|---|---|---|---|---|
| `repo_dir` | string | `/opt/project` | `--repo-dir` | The clone. Relative paths are joined to the current directory. Cloned and synced by jobs, never at start. |
| `repo_url` | string | `git@git.example.com:vdp/dbt-vdp.git` | | The URL the clone action proposes (`defaults.repo_url` in the status); the source for deriving GitLab settings. Shown in `GET /status` once cloned: keep credentials out. |
| `bind` | string | `0.0.0.0` | `--bind` | Listen address, joined with the port as `bind:port`. |
| `port` | u16 | `8765` | `--port` | `0`: a free port (the banner names it). |
| `cors_origin` | string | `""` | `--cors-origin` | One origin for `Access-Control-Allow-Origin` (with credentials). Empty: no CORS headers. |
| `ui_dir` | string | `""` | `--ui-dir` | Built frontend. Empty: API only. Must hold `index.html` or the start fails (exit 2). Relative paths are joined to the current directory. |
| `validate` | bool | `false` | `--validate` | Build a validator that runs `[release].validate` through the dbt engine in the clone before a release push. Either the flag or the setting turns it on. Needs the dbt engine (`vdp env doctor`). |
| `keep_jobs` | usize | `50` | | Jobs kept. The oldest job that is not running is dropped when a job starts. At least 1. |
| `git_timeout_secs` | u64 | `600` | | A git command running longer is killed. `0`: no limit. |
| `conflict_timeout_secs` | u64 | `7200` | | A waiting conflict with no decision for this long aborts the run, nothing pushed. Read from the code: `0` aborts at the first conflict. |
| `state_dir` | string | `/var/lib/vdp-hub` | | Where `analyses.json` and `jobs.json` are written (atomically). Empty: memory only. A write error is a warning in the log, not an error. |
| `auth` | string | `gitlab` | | `gitlab` or `none`; anything else is an error at start (exit 2). `none` logs a warning: anyone who reaches the hub can release, and commits use the clone's identity. |
| `gitlab_url` | string | `""` | | GitLab base URL (the hub calls `{gitlab_url}/api/v4`). Empty: derived. |
| `gitlab_project` | string | `""` | | `group/project`. Empty: derived. |
| `push_url` | string | `""` | | Where pushes go (with the signed-in person's token). Empty: derived. |
| `<name>` (a table) | table | | | Settings of the extension `<name>`: `HubSettings::extension::<T>("<name>")` deserialises `[release_hub.<name>]` into `T` (its default when absent; an error naming `[release_hub.<name>]` when it does not fit). |

**Deriving the GitLab settings** (`GitLab::from_repo_url`, then each non-empty explicit key overrides one value; all three must be non-empty
afterwards, else the start fails with `... needs gitlab_url, gitlab_project and push_url (not derivable from <repo_url>)`; with `auth = "none"` none of this runs):

| `repo_url` | `gitlab_url` | `gitlab_project` | `push_url` |
|---|---|---|---|
| `git@git.example.com:vdp/dbt-vdp.git` | `https://git.example.com` | `vdp/dbt-vdp` | `https://git.example.com/vdp/dbt-vdp.git` |
| `ssh://git@git.example.com:2222/vdp/dbt-vdp.git` | same | same | same |
| `https://git.example.com/vdp/dbt-vdp[.git]` | same | same | same |
| a path on disk (`/tmp/origin.git`) | not derivable: set all three | | |

The result always has the scheme `https`. An `http://` `repo_url` is not among the tested forms and, read from the code, yields a wrong host
(`http` is taken for the host of an scp-like URL): set the three keys explicitly (see "Pointing it at the workshop" in the README).

**Unknown keys.** `HubSettings` has `#[serde(flatten)] extensions: BTreeMap<String, Value>`, so a mistyped key is not rejected: it lands in
`extensions`. `vdp-release-ui` (factory `no_extensions`) warns `[release_hub.<key>] is set, but this binary serves no such release extension`
for every such key; a binary with an extension warns for the segments it does not serve. Read the start log after editing the file.

## `[notify]`

| Key | Type | Default | Effect |
|---|---|---|---|
| `smtp_host` | string | `smtp.gmail.com` | SMTP server. Empty: not ready (`[notify].smtp_host is empty`). |
| `smtp_port` | u16 | `587` | |
| `security` | `starttls` \| `tls` \| `none` | `starttls` | `starttls`: `starttls_relay`; `tls`: implicit TLS (`relay`, usually 465); `none`: no encryption (`builder_dangerous`), for a local relay or a test server. Credentials are still used when both secrets exist. |
| `from` | string | `""` | Sender. Empty: the secret named by `from_secret`, else the SMTP user. |
| `user_secret` | string | `SMTP_USER` | Name of the secret with the SMTP account. |
| `password_secret` | string | `SMTP_PASSWORD` | Name of the secret with its password. |
| `from_secret` | string | `SMTP_FROM` | Name of the secret with the sender. |
| `default_recipients` | list of string | `[]` | Recipients proposed to the UI (`defaults.recipients` in `GET /status`). |
| `timeout_secs` | u64 | `15` | SMTP connection timeout. |

Readiness (`SmtpMailer::readiness`, reported as `smtp` in `GET /status`): ready when `smtp_host` is non-empty, **and** (`security = "none"` or both
user and password secrets resolve), **and** a sender is known. When not ready, `send` returns `Delivery::PreviewOnly { reason }` (the `reason` is the
readiness text) and sends nothing; with no recipients it fails first with `NoRecipients` (400), also in preview-only mode. An invalid address is a 400 only when mail is really sent: in preview-only mode no address check runs. An SMTP failure is a 500.

## `[release]` and `[jira]`

| Key | Default | Used by the hub for |
|---|---|---|
| `[release].ticket_prefix` | `INTEL` | recognising tickets in commit messages (`defaults.ticket_prefix`) |
| `[release].from` | `origin/dev` | the source of releases and patches; its local name (`dev`) is the branch the hub never writes; the clone detaches there |
| `[release].to` | `origin/main` | where a release is compared and promoted; its local name (`main`) is written by promotions only; also the branch whose force-push right is checked at sign-in |
| `[release].branch_prefix` | `release/` | release branches; also the `release/*` rule whose force-push right is checked at sign-in (sample branch `release/2099_DROP99`) |
| `[release].validate` | `["compile", "--no-partial-parse"]` | the dbt arguments of the pre-push validation |
| `[jira].base_url` | `https://issuetracker.example.com` | ticket lookups and the ticket URLs in announcements |
| `[jira].user_secret`, `token_secret` | `JIRA_USER`, `JIRA_TOKEN` | the Jira account and API token |
| `[jira].excluded_fix_versions` | `["Nog in te plannen"]` | fix versions that take a ticket out of a release |
| `[jira].sprint_field` | `customfield_11221` | the custom field with the sprints |
| `[jira].advance_transition` | `""` | not used by the hub (`vdp jira advance`) |

## Secrets

The hub never takes credentials from the browser except the GitLab token of a signing-in person. Server-side secrets, by default name:

| Secret | Used for | When missing |
|---|---|---|
| `JIRA_USER`, `JIRA_TOKEN` | the Jira client | start logs `Jira is not available, analyses will lack Jira data`; `GET /status` `jira.ready = false`; analysis steps log a warning; tickets carry a `jira_error` |
| `SMTP_USER`, `SMTP_PASSWORD` | SMTP login | `smtp.ready = false`; announcements are previewed |
| `SMTP_FROM` | the sender (when `[notify].from` is empty) | falls back to `SMTP_USER` |

They are resolved by `vdp-secrets` from the sources and in the order of `[secrets].precedence` (default, highest priority first: `vault:vdp/airflow`, `dotenv`,
`vault:vdp/{env}/dbt`, `env`; `{env}` is the lower-case environment, `{env_short}` its short name), where `dotenv` is the file `[core].dotenv` (default
`.devcontainer/.env`, relative to the dbt project) and `env` is the process environment. `--no-vault` / `$VDP_NO_VAULT` skips Vault. A secret
provider that cannot be set up stops the start (exit 2); a single missing secret does not.

## Not configurable (constants in the code)

| Value | Where |
|---|---|
| session lifetime 12 h, absolute | `auth::SESSION_HOURS` |
| request body cap 1 MiB | `http::MAX_BODY` |
| diff and conflict-version cap 256 KiB | `hub/mod.rs` (`change_file`, `conflict_versions`) |
| GitLab request timeout 20 s, answer cap 4 MiB, protected branches read 100 per page, at most 50 pages | `auth::GitLab::get`, `rights` |
| state file names `analyses.json`, `jobs.json` | `hub/mod.rs`, `jobs.rs` |
| API prefix `/api/v1`, cookie name `vdp_session` | `http::API`, `http::SESSION_COOKIE` |

## Adding a setting

1. Add the field to the settings struct with its doc comment (the struct is `#[serde(default)]`; `NotifySettings`, `ReleaseSettings` and `JiraSettings` are
   also `deny_unknown_fields`) and to its `Default` impl.
2. Wire it where it is used: `hub::serve` builds `HubConfig` from `HubSettings`; a new `release serve` option needs a field in `ServeArgs`
   (`args.rs`), its `long_help` constant in `help.rs`, and a line in `SERVE_LONG_ABOUT` if it matters.
3. Update this file and the README tables. A setting of an extension belongs to that extension's own struct, not to `HubSettings`.
