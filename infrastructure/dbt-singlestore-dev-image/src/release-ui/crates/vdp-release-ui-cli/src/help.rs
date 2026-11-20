//! The long help (`--help`) of `vdp-release-ui`: what it adds to `vdp`, and `release serve` in detail.
//!
//! The short help (`-h`) is the one-line `about` and `help` in [`crate::args`]. The texts are wrapped by hand (clap
//! does not wrap without its `wrap_help` feature): lines of a `long_about` or `after_long_help` stay within 96
//! columns, lines of an option's `long_help` within 86 (clap indents those by 8).

pub(crate) const ROOT_LONG_ABOUT: &str = "\
The vdp toolkit with the release UI's backend.

`vdp-release-ui` is the whole `vdp` command line (the dbt wrapper, sources, releases, Jira, SQL,
quality checks, env, dev) plus one command of its own: `release serve`, which starts the release
hub. The hub is the HTTP API behind the release UI, the web application release managers use to
analyse, cut, patch and promote DROP releases. Every other command works as in `vdp`, with the
same options and help (`vdp-release-ui <command> --help`).";

pub(crate) const ROOT_AFTER_LONG_HELP: &str = "\
What this binary adds to `vdp`:
  release serve  start the release hub: the HTTP API (and optionally the built UI) over one clone
                 of the dbt repository, running the release flows of `vdp release ...` for the
                 browser, with GitLab sign-in and the release announcement by mail

Environments (--env):
  LDV, ABT, CBT, PRD, or a name configured as [environments.<name>]. `dev`, `val` and `prd` are
  aliases of LDV, ABT and PRD (CBT has none). Without --env the environment is $DBT_DEPLOY_TARGET,
  else [core].default_environment (LDV unless configured). An unknown name stops the command with
  exit code 2.

Configuration:
  The same vdp.toml as `vdp`, layered the same way, later files winning key by key:
    built-in defaults < /etc/vdp/vdp.toml < <dbt project>/vdp.toml < --config FILE ($VDP_CONFIG)
  The dbt project is --project-dir ($VDP_PROJECT_DIR), else the nearest parent directory of the
  current one that has a dbt_project.yml. `release serve` reads [release_hub] and [notify] besides
  the sections of the release flows ([release], [jira]); secrets come from Vault, the dotenv file
  and the environment in the order of [secrets].precedence (--no-vault skips Vault).

Exit codes:
  0  success
  1  the command ran, but some of its work failed: a batch with failures, a failed check
  2  setup or usage error: bad arguments, unreadable configuration, unknown environment, a
     service that cannot be reached, a refused operation
  `release serve` runs until it is stopped; it exits with 2 when it cannot start.

Logging:
  Logs go to stderr: info by default, debug with -v, warnings and errors only with -q. $VDP_LOG (a
  tracing filter such as `debug`) overrides the level. --log-json FILE also writes the events as
  one JSON object per line: info and above (debug with -v), whatever -q says. The line that says
  where the hub listens is printed under -q as well.

Examples:
  # Start the hub with the [release_hub] settings
  vdp-release-ui release serve

  # Any command of vdp works here too
  vdp-release-ui --env prd release analyse --fixversion 2026_DROP05 --json

  # The same hub, started through the vdp binary
  vdp release-ui release serve --port 8765";

pub(crate) const SERVE_LONG_ABOUT: &str = "\
Serve the release hub: the HTTP API (and optionally the built UI) over one clone of the repository.

The hub is the server behind the release UI. It runs the release flows of `vdp release ...` for the
browser: analyses stored with their selection, git work as jobs that run one at a time with a step
log, sign-in with a personal GitLab token ([release_hub].auth = \"gitlab\", or \"none\" for local
use), and the release announcement by mail ([notify], over SMTP). Credentials never reach the
browser: Jira and SMTP come from the secrets. The API is under `/api/v1`.

Every option defaults to its [release_hub] setting. Once it listens, the command prints
`release hub on http://<address>/api/v1 (repository <dir>[, UI from <dir>])` on stderr (also with
-q) and runs until it is stopped.

Reads: [release_hub] (repo_dir, repo_url, bind, port, cors_origin, ui_dir, validate, auth,
state_dir, timeouts, ...), [notify], [release], [jira], and the secrets for Jira and SMTP. A
missing Jira connection is a warning: analyses then lack Jira data.
Changes: nothing when it starts. What users do in the UI does: the clone at --repo-dir is cloned
and synced, releases and patches are pushed to origin, announcements are mailed, and analyses and
job history are kept in [release_hub].state_dir (default /var/lib/vdp-hub; empty: in memory).
It has no --dry-run.

Exit codes: it runs until stopped. Exit code 2 when it cannot start: invalid configuration, a
--ui-dir without index.html, secrets that cannot be resolved, an address it cannot listen on.";

pub(crate) const SERVE_AFTER_LONG_HELP: &str = "\
Examples:
  # The hub with the [release_hub] settings
  vdp-release-ui release serve

  # Local use: the built frontend next to the API, on loopback only
  vdp-release-ui --env dev release serve --bind 127.0.0.1 --port 8765 \\
    --ui-dir ../release-ui-frontend/build

  # A frontend started with `npm run dev`, on another origin
  vdp-release-ui release serve --cors-origin http://localhost:5173

  # Through the vdp binary, validating releases with dbt before they are pushed
  vdp release-ui release serve --validate";

pub(crate) const SERVE_REPO_DIR_LONG: &str = "\
The clone of the dbt repository the hub analyses and releases from. Default:
[release_hub].repo_dir (`/opt/project`). A relative path is taken from the current
directory. The clone is created and synced by jobs of the UI.";

pub(crate) const SERVE_BIND_LONG: &str = "\
The address to listen on. Default: [release_hub].bind (`0.0.0.0`, every interface).
Use `127.0.0.1` for local use.";

pub(crate) const SERVE_PORT_LONG: &str = "\
The port to listen on. Default: [release_hub].port (8765). `0` picks a free port: the
line the command prints when it listens tells which.";

pub(crate) const SERVE_UI_DIR_LONG: &str = "\
The built frontend (a directory with an `index.html`; the command refuses to start
without one). The hub serves its files next to the API and answers unknown paths with
`index.html`, as a single-page app needs. Default: [release_hub].ui_dir; when that is
empty too, the hub is API only: serve the frontend some other way (a web server or
proxy in front).";

pub(crate) const SERVE_CORS_ORIGIN_LONG: &str = "\
Allow a frontend served from this origin, by answering with it as
`Access-Control-Allow-Origin`, e.g. `http://localhost:5173` for a frontend run with
`npm run dev`. Default: [release_hub].cors_origin; when that is empty too, only the
same origin.";

pub(crate) const SERVE_VALIDATE_LONG: &str = "\
Validate the releases the hub builds before they are pushed: dbt runs
[release].validate (default `compile --no-partial-parse`) in the clone, as `release
create` does. Also on when [release_hub].validate is true. It needs the dbt engine
(see `env doctor`).";
