# Commands, flags, exit codes, environment, files, reports

The command line of `vdp-vaultspeed`, checked against `crates/vdp-vaultspeed-cli/src/args.rs`, `help.rs`, the
command code and the binary's `--help` pages. This file is the single source; `README.md` carries the same tables.
The text of every page is in `crates/vdp-vaultspeed-cli/src/help.rs`: edit it there, not here.

## Shape

```
vdp-vaultspeed [global options] <command> ...
  vaultspeed [--framework-dir DIR] plan | deploy | ledger | rules | dags | history     (this workspace)
  release serve                                                                       (adds the VaultSpeed hub extension)
  dbt | sources | release | jira | sql | qa | env | dev                                (the whole `vdp` command line, unchanged)
```

`vdp vaultspeed ...` (in `vdp-rust-utils`) hands over to `vdp-vaultspeed vaultspeed ...` with the same arguments
(`HANDOFFS` in `vdp-rust-utils/crates/vdp-cli/src/external.rs` keeps the command name; the binary is found next to
`vdp` or on `PATH`, else `vdp` exits 2). Inside `vdp-vaultspeed` the hand-off command itself is refused.

## Global options (shared with `vdp`)

| Option | Environment | Meaning |
|---|---|---|
| `--env <ENV>` | | `LDV`, `ABT`, `CBT`, `PRD`, or a name configured as `[environments.<name>]`; `dev`, `val`, `prd` are aliases of `LDV`, `ABT`, `PRD` (`CBT` has none). Unknown name: exit 2 |
| `--project-dir <DIR>` | `VDP_PROJECT_DIR` | the dbt project (nearest parent with `dbt_project.yml`); its `vdp.toml` and dotenv file are read |
| `--config <FILE>` | `VDP_CONFIG` | extra TOML, must exist, layered last |
| `--target <TARGET>`, `--profiles-dir <DIR>` | | dbt target and profiles directory for the toolkit's own connections and dbt runs; the `vaultspeed` commands connect through the secrets, not the profile |
| `--no-vault` | `VDP_NO_VAULT` (`1/true/yes/on`) | do not log in to Vault; secrets come from dotenv and the process environment |
| `-v`, `-q` | `VDP_LOG` (a tracing filter, overrides both) | debug logging / warnings and errors only, on stderr |
| `--log-json <FILE>` | `VDP_LOG_JSON` | also write NDJSON events (info and above, debug with `-v`, whatever `-q` says) |

Configuration layers, later wins key by key: built-in defaults < `/etc/vdp/vdp.toml` < `<dbt project>/vdp.toml` <
`--config FILE`. **A framework checkout is not a dbt project**: a `vdp.toml` lying in a directory without
`dbt_project.yml` is not read (checked: a bad key in it was ignored). Give the `[vaultspeed*]` sections through
`--config`/`$VDP_CONFIG`, `/etc/vdp/vdp.toml`, or the `vdp.toml` of a dbt project named by `--project-dir`.
All `[vaultspeed*]` structs reject unknown keys (exit 2 with the key named).

## Environment and checkout resolution

| What | Order |
|---|---|
| Environment of a `vaultspeed` command | `--env`, else `$TARGET_BRANCH` (the framework's CI sets it to the environment branch; must name an environment, else exit 2), else `$DBT_DEPLOY_TARGET`, else `[core].default_environment` (`LDV` unless configured). Other commands skip `$TARGET_BRANCH` |
| Framework checkout | `--framework-dir` (`$VDP_VAULTSPEED_DIR`), else `[vaultspeed].project_dir` (default `.`), relative to the current directory |
| History file | `--path`, else `[vaultspeed.history].path` below the checkout |

Environment names resolve as in `vdp`: `dev` is `LDV`, so `[vaultspeed.environments.dev]` configures `LDV` (merged
key by key with an `LDV` section, the alias wins a conflict). Other names are custom environments, ordered after the
built-in ones. `$CI_COMMIT_SHA`, when set, is stamped on every ledger entry and history row as `git_sha`.

## `vaultspeed` commands

`--framework-dir <DIR>` (`VDP_VAULTSPEED_DIR`) is accepted by every `vaultspeed` command (given before or after the
subcommand).

### `plan` and `deploy`

`plan` is `deploy --preview`. Both take the same options:

| Option | Meaning |
|---|---|
| `--offline` | only the JSONL ledger `vaultspeed/ledger/<ENV>.jsonl`: no database, no Vault, no Airflow (dotenv and process environment are still read for `SINGLESTORE_HOST`, used in DAG dataset URIs; a placeholder otherwise). `deploy --offline` needs `--preview` (exit 2 otherwise: use `plan --offline`) |
| `--files <GENERATION>...` | only these generations, by label (the zip file name without `.zip`, e.g. `1134_FMC 1133_DDL`); all values up to the next option are labels; staged PML always runs; a label that is not staged is an error (exit 2) |
| `--preview` (alias `--dry-run`) | plan and render, change nothing: no statement runs, no ledger, `deployed/`, archive, history, DAG save or upload |
| `--explain` | print the decision of every group and the diff of every rendered DAG |
| `--report-out <FILE>` | also write the full report (the plan, in preview) as JSON to this file (relative to the current directory) |
| `--junit <FILE>` | also write JUnit XML (GitLab test reports) |

`deploy` finds the staged PML scripts and generation zips, plans each statement against the ledger, runs what is due,
records every statement, renders and publishes the DAGs of FMC generations, tidies the staged files and writes the
reports. It connects to SingleStore, and uploads DAGs over SFTP when an `AIRFLOW_URL`/`AIRFLOW_HOST` secret exists
(otherwise they are only rendered into the checkout, with a warning). `plan` still reads the ledger and the catalog
from the database unless `--offline`; it writes only the reports (the JSON report in `<checkout>/logs/`).

### `ledger`

| Command | Meaning |
|---|---|
| `ledger import [--offline] [--force]` | seed the ledger from the legacy `vaultspeed/deployed/{PML,DDL,ETL}` files as `imported` entries (source `legacy:deployed`); prints files and statements per artifact kind. Refused (exit 2) when the ledger already has entries unless `--force`. `--offline`: JSONL only |
| `ledger stats [--offline]` | entries and last sequence of the environment's ledger, entries by outcome and by source. Read-only |

### `rules test`

`rules test <FILE> [--artifact ddl|etl]` (default `ddl`) runs a SQL file through the pipeline a deployment uses for the
kind and prints, per statement, `#<n> <kind> <target> rules: [<ids>] raw <12-char fingerprint>` and a unified diff from
raw to effective text. DDL is split, stripped of comments (unless a `-- , SHARD KEY` hint is present) and rewritten by
the DDL rules; ETL is split and rewritten by the ETL rules. Reads the file and `vaultspeed/staging/manual_changes/`;
no database, no Vault. Exit 2 when the file or a rule file cannot be read or parsed.

### `dags render`

`dags render <PATH> [--host <HOST>]`: `PATH` is a generation zip, a directory or one `.py` file (only `.py` files are
rendered). Per DAG it prints `# <name> -> <published name> [<transformations>]` and a unified diff original to rendered.
A DAG that cannot be parsed or altered is reported as an error and the others still render (exit 1). `--host`
(`$SINGLESTORE_HOST`, process environment only) is the host embedded in dataset URIs; default the placeholder
`singlestore-rel1<env>.invalid` (`<env>` lower-case). Exit 2 when the path cannot be read. Nothing is published.

### `history`

`--path <FILE>` and `--json` are accepted by both subcommands. Both read the file for the selected environment, never
create it, and exit 2 when there is no file yet.

| Command | Meaning |
|---|---|
| `history object <IDENTIFIER> [--all]` | the statements of the object that is, or was last, named `database.name`, oldest first, across renames: run time, `operation/statement_kind`, the identifier at the time, outcome, rows, duration. `--all`: every lifespan that used the name instead. Exit 1 when the name was never seen |
| `history objects [--status live\|dropped]` | the objects: id, type, identifier, first identifier, status, origin, created, ended. Empty history prints nothing (`[]` with `--json`) |

## `release serve`

Starts the release hub of `release-ui` with the VaultSpeed extension (`[release_hub.vaultspeed] enabled = false`
serves the dbt releases only). Every option defaults to its `[release_hub]` setting; it runs until stopped and exits 2
when it cannot start (invalid configuration, `--ui-dir` without `index.html`, unresolved secrets, address in use).

| Option | Meaning |
|---|---|
| `--repo-dir <DIR>` | the clone of the dbt repository (`[release_hub].repo_dir`, `/opt/project`); the VaultSpeed clone is `[release_hub.vaultspeed].repo_dir` |
| `--bind <ADDRESS>`, `--port <PORT>` | listen address (`0.0.0.0`) and port (`8765`; `0` picks a free one) |
| `--ui-dir <DIR>` | the built frontend; without it the hub is API only |
| `--cors-origin <ORIGIN>` | allow a frontend from this origin |
| `--validate` | validate dbt releases before pushing (needs the dbt engine) |

Routes of the VaultSpeed side: `release-model.md`. There is **no CLI for VaultSpeed releases**: they exist only
through the hub.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | the command ran but its work failed: statements that failed or were refused (a refused group counts as failed), a source that could not be loaded, a DAG that did not render or publish, `history object` for a name that was never seen |
| 2 | setup or usage error: bad arguments or unknown environment, unreadable or invalid configuration (unknown key, empty `rdv_order_numbers`), missing secrets, a connection that fails, `--files` label not staged, `deploy --offline` without `--preview`, `ledger import` into a non-empty ledger, no history file, a ledger that cannot be written (the run stops and writes no report) |

A run with failures is `Outcome::Failures` (exit 1), not an error.

## Secrets

Resolved through `[vaultspeed].secrets_precedence` (first source that knows a key wins). Vault is read only when a
`vault:` source is listed, no `--no-vault`, and AppRole credentials exist; a Vault failure only warns unless
`[secrets].vault_required`. `--offline` never reads Vault.

| Need | Names (first set wins; two synonyms in one source are an error) |
|---|---|
| SingleStore host | `SINGLESTORE_HOST`, `SINGLESTORE_SERVER`, `SINGLESTORE_URL` |
| SingleStore user | `SINGLESTORE_USER`, `SINGLESTORE_USERNAME` |
| SingleStore password | `SINGLESTORE_PASS`, `SINGLESTORE_PASSWORD`, `SINGLESTORE_PWD` |
| SingleStore database, port | not secrets: `[vaultspeed.environments.<ENV>].database` and `.singlestore_port`; a `SINGLESTORE_DB`/`_PORT` secret is ignored |
| Airflow host (DAG upload) | `AIRFLOW_URL`, `AIRFLOW_HOST` (scheme and `:port` allowed; unset: DAGs are only rendered locally) |
| Airflow private key | `private_key`, `AIRFLOW_PRIVATE_KEY` (PEM; written to a 0600 temp file for the run; unset: ssh agent or the user's keys) |
| Vault AppRole role id | `KEYVAULT_ROLE_ID`, else `KEYVAULT_ROLE_ID_LDV_ABT` (`LDV`, `ABT`, `CBT`) or `KEYVAULT_ROLE_ID_CBT_PRD` (`PRD`) |
| Vault AppRole secret id | `KEYVAULT_SECRET_ID`, else `KEYVAULT_SECRET_ID_LDV_ABT` or `KEYVAULT_SECRET_ID_CBT_PRD`, as above |

The AppRole variables are read from the dotenv file or the process environment, never from Vault. They are the defaults of
`role_id_vars` and `secret_id_vars` in the shared `[environments.<ENV>]` section of `vdp-core` (as are `vault_url` and the CA
file `ca_certificate`, looked up in `[vaultspeed.paths].assets_dir`); `CBT` uses the non-production set (`_LDV_ABT`) despite
the `_CBT_PRD` names (`vdp-rust-utils/crates/vdp-core/src/config.rs`). Without credentials Vault is skipped (debug log only);
a failed login only warns; with `[secrets].vault_required` both are errors (exit 2).

The SSH user is always `[vaultspeed.dags].ssh_user` (no secret overrides it; see `engine.md`). Host keys are checked
strictly (`known_hosts` of the user, or `[vaultspeed.dags].known_hosts`).

## Environment variables

Every variable the binary reads, in one table (the secrets also come from Vault and the dotenv file, see above).

| Variable | Read by | Effect |
|---|---|---|
| `TARGET_BRANCH` | `vaultspeed` commands | the environment when `--env` is absent (set by the framework's CI; must name an environment, else exit 2) |
| `DBT_DEPLOY_TARGET` | every command | the environment when `--env` and `TARGET_BRANCH` are absent |
| `VDP_VAULTSPEED_DIR` | `vaultspeed` commands | `--framework-dir` |
| `VDP_PROJECT_DIR`, `VDP_CONFIG` | every command | `--project-dir`, `--config` |
| `DBT_PROFILES_DIR` | toolkit's own dbt use | first candidate for the profiles directory (`--profiles-dir` wins) |
| `VDP_NO_VAULT` | every command | `--no-vault` (`1`, `true`, `yes`, `on`) |
| `VDP_LOG`, `VDP_LOG_JSON` | every command | tracing filter (overrides `-v`/`-q`), NDJSON log file (`--log-json`) |
| `VDP_ADBC_DRIVER` | commands that connect | path of the SingleStore ADBC driver (else `[core].adbc_driver`, else `lib/dbt/` above the dbt executable or the binary, else the library search path) |
| `CI_COMMIT_SHA` | `plan`, `deploy` | stamped as `git_sha` on ledger entries and history rows |
| `SINGLESTORE_HOST` | `dags render` | default of `--host` (process environment only); also the host secret |
| `SINGLESTORE_HOST`/`SERVER`/`URL`, `SINGLESTORE_USER`/`USERNAME`, `SINGLESTORE_PASS`/`PASSWORD`/`PWD` | commands that connect | the secrets of the table above (through `secrets_precedence`) |
| `AIRFLOW_URL`, `AIRFLOW_HOST`, `AIRFLOW_PRIVATE_KEY` (secret `private_key`) | `deploy` | DAG upload (through `secrets_precedence`) |
| `KEYVAULT_ROLE_ID`, `KEYVAULT_SECRET_ID` and their `_LDV_ABT` / `_CBT_PRD` variants | when a `vault:` source is read | Vault AppRole login (dotenv or process environment only) |

## Files of a checkout

Below `[vaultspeed.paths].vaultspeed_dir` (default `vaultspeed/`) unless noted.

| Path | Role |
|---|---|
| `staging/pre_model_input/*.sql` | PML scripts; one source, runs first, every time it is staged; a completed script moves to `deployed/PML/`, one that failed or was not reached stays staged |
| `staging/release/<n>_<KIND>.zip` | generation zips (`DDL`, `GENERATE_ETL`/`ETL`, `COMP`, `FMC`); run in numeric order; a processed zip moves to `deployed/archive/` (kept staged after failures unless `archive_on_failure`) |
| `staging/manual_changes/DDL/*.json`, `.../ETL/*.json` | rewrite rules, every `*.json` in file-name order (typically `QUERY_MODIFICATIONS.json`) |
| `ledger/<ENV>.jsonl` | the JSONL ledger (mirror of the database ledger; the ledger itself with `--offline` or `backend = "jsonl"`) |
| `deployed/{DDL,ETL,PML}/<dir>/<JOB>.sql` | readable mirror of what ran (documentation, never state); DDL appends, ETL and PML overwrite by default |
| `airflow/dags/`, `airflow/meta/` | rendered DAGs and the metadata files they read |
| `logs/<source>/deploy_errors_<yy-mm-dd@HH:MM:SS>.csv` | failed statements of a source: `JOB_NAME, TYPE, JOB_QUERY, ERROR_MESSAGE`, all quoted |
| `<checkout>/logs/vsaf_<ENV>_<run id>.json` | the full JSON report of every run (outside `vaultspeed/`) |
| `<checkout>/config/vaultspeed.json` | source list (`FMC_SOURCES[].SOURCE_NAME`) used to name the source of a DAG |
| `<checkout>/.vdp/vaultspeed-history.duckdb` | the object history (local, do not commit) |
| `<checkout>/.devcontainer/.env`, `<checkout>/assets/` | dotenv file and CA certificates |

Zip layout: `DDL` and `GENERATE_ETL`/`ETL` zips take every `.sql` in them; `COMP` takes `DDL/` and `ETL/` folders
(its DAGs too when `publish_comp_dags`); `FMC` takes top-level `.py` DAGs and `.json` metadata.

## Reports

| Report | Where | Content |
|---|---|---|
| console | stdout | header, per source one line `executed/failed/refused/skipped by reason`, a panel per failed statement, a totals table (`applied`, `satisfied`, `failed`, `executed_groups`, `skipped_groups`, `dags`); `--explain` adds every decision and DAG diff. In a preview `applied` counts what would run |
| JSON | `<checkout>/logs/vsaf_<ENV>_<run id>.json`, plus `--report-out` | every decision, statement and DAG diff; in a preview it is the execution plan |
| CSV | `vaultspeed/logs/<source>/deploy_errors_*.csv` | failed statements per source |
| JUnit XML | `--junit FILE` | one suite per source; one test case per group (marked skipped, refused or failed), per DAG and per source-level error |
