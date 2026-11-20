# vaultspeed-framework

The deployer of the VaultSpeed Automation Framework, ported to Rust, as a Cargo workspace of nine crates. It deploys what
VaultSpeed generates (hand-written PML scripts and generation zips) from a framework checkout to SingleStore and Airflow,
keeps a ledger of what it ran, keeps a local history of every database object it changed, and adds the VaultSpeed
releases to the release hub.

Contents: [1 What it is](#1-what-it-is-and-where-it-sits) | [2 Build and run](#2-build-install-run) |
[3 Commands](#3-command-reference) | [4 Configuration](#4-configuration-reference) |
[5 Crates](#5-crates-and-architecture) | [6 Planning, ledger, history](#6-planning-ledger-and-object-history) |
[7 Releases and the hub](#7-vaultspeed-releases-and-the-hub-extension) | [8 The workshop](#8-using-it-with-the-workshop) |
[9 Testing and verification](#9-testing-and-verification) | [10 Known gaps](#10-known-gaps)

For work in this folder read `CLAUDE.md` and `.agents/skills/vdp-vaultspeed-framework/SKILL.md`; the skill's
`.agents/skills/vdp-vaultspeed-framework/references/` (`commands.md`, `config.md`, `engine.md`, `release-model.md`) carry the
same command and configuration tables plus the engine internals and the release model.

## 1. What it is and where it sits

| In | Out |
|---|---|
| PML scripts `vaultspeed/staging/pre_model_input/*.sql`; generation zips `vaultspeed/staging/release/<n>_<KIND>.zip` (`DDL`, `GENERATE_ETL`, `COMP`, `FMC`); rewrite rules `vaultspeed/staging/manual_changes/{DDL,ETL}/*.json`; the source list `config/vaultspeed.json` | DDL and ETL statements run on SingleStore; a ledger (a table on the environment's SingleStore server plus a JSONL mirror, or JSONL only); Airflow DAGs rendered into the checkout and uploaded over SFTP; reports (console, JSON, CSV of failures, JUnit XML); a local DuckDB object history |

It is called by a person, by the framework's own CI (the environment branch arrives as `TARGET_BRANCH`, GitLab provides
`CI_COMMIT_SHA`, and `--junit` feeds GitLab's test reports), by `vdp vaultspeed ...` (the `vdp` command line hands over to
this binary), and as `release serve`, the release hub with the VaultSpeed extension.

One binary, `vdp-vaultspeed` (crate `vdp-vaultspeed-cli`): the whole `vdp` command line (`dbt`, `sources`, `release`, `jira`,
`sql`, `qa`, `env`, `dev`), the `vaultspeed` commands, and `release serve`.

```
vdp-rust-utils  <--  release-ui  <--  vaultspeed-framework        "a <-- b": b uses a
vdp toolkit          release hub       deployer, history, releases
(vdp)                (vdp-release-ui)  (vdp-vaultspeed)
```

All dependencies between the three are `path` dependencies (`../vdp-rust-utils/crates/<name>`,
`../release-ui/crates/<name>`), so the folders must stay side by side under `infrastructure/dbt-singlestore-dev-image/src/`.
No code is copied between them.

**This workspace is not part of the workshop stack.** Nothing in the stack references it: no VaultSpeed content in
`dbt-singlestore-workshop/`, no `[vaultspeed*]` section in its `vdp.toml`, no compose service, Makefile target, CI job, nginx
route or overview entry, and no image contains the binary (the dev image builds only `-p vdp-cli` from `vdp-rust-utils`).
Section 8 says what would not fit if someone tried.

## 2. Build, install, run

```
cd infrastructure/dbt-singlestore-dev-image/src/vaultspeed-framework
cargo build --release --locked -p vdp-vaultspeed-cli      # target/release/vdp-vaultspeed
```

Needs `../vdp-rust-utils` and `../release-ui` side by side and Rust 1.96 (`rust-toolchain.toml`, with rustfmt and clippy).
DuckDB is compiled from source (`bundled`, C++) and tree-sitter compiles C, so a C and C++ toolchain is needed and the first
build takes long. Disk: `target/` measured about 13 GB after a debug build plus a test build, and the debug binary alone is
1.1 GB; a release build is smaller but slow (thin LTO, one codegen unit).

Put `vdp-vaultspeed` next to `vdp` or on `PATH`: `vdp vaultspeed ...` then replaces itself with
`vdp-vaultspeed vaultspeed ...` (the table `HANDOFFS` in `vdp-rust-utils/crates/vdp-cli/src/external.rs`; without the binary
`vdp` says so and exits 2).

At run time:

- SingleStore is reached through the ADBC driver that `vdp` uses: `$VDP_ADBC_DRIVER`, else `[core].adbc_driver`, else
  `lib/dbt/` above the dbt executable or the binary, else the library search path.
- The DAG upload runs the system `sftp` (OpenSSH) with strict host keys and batch mode.

Try it without any service:

```
vdp-vaultspeed vaultspeed rules test file.sql                      # how a SQL file is split, rewritten and classified
vdp-vaultspeed vaultspeed dags render fmc.zip                      # the DAG diffs of an FMC zip, a directory or a .py file
vdp-vaultspeed --env LDV vaultspeed plan --offline --explain       # what a deploy would run, from the JSONL ledger
```

`rules test` and `dags render` change nothing; `plan --offline` writes only its reports (the JSON report in `logs/`). A missing
`vaultspeed/ledger/<ENV>.jsonl` counts as an empty ledger (nothing is created for it), and `--no-vault` is redundant here:
none of the three reads Vault (offline never does; the other two read no secrets). All three ran this way in a throwaway
checkout.

Settings are the `vdp.toml` family. **A framework checkout is not a dbt project**, and a `vdp.toml` lying in a directory
without `dbt_project.yml` is not read (checked: a bad key in it was ignored). Put the `[vaultspeed*]` sections in a file
given with `--config` (`$VDP_CONFIG`), in `/etc/vdp/vdp.toml`, or in the `vdp.toml` of a dbt project named by `--project-dir`.

## 3. Command reference

```
vdp-vaultspeed --env LDV vaultspeed plan                       # what a deploy would run, changing nothing
vdp-vaultspeed --env LDV vaultspeed deploy                     # deploy the staged PML and generations
vdp-vaultspeed --env LDV vaultspeed ledger import | stats      # seed the ledger from `deployed/`, or summarise it
vdp-vaultspeed vaultspeed rules test file.sql                  # split, rewrite, classify one SQL file
vdp-vaultspeed vaultspeed dags render fmc.zip                  # the DAG diffs of a zip, a directory or a .py file
vdp-vaultspeed --env LDV vaultspeed history object edw.hub_x
vdp-vaultspeed --env LDV vaultspeed history objects --status dropped
vdp-vaultspeed release serve                                   # the release hub, with the VaultSpeed releases
```

### Global options (shared with `vdp`)

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
`--config FILE`. All `[vaultspeed*]` structs reject unknown keys (exit 2, the key is named).

### Environment and checkout resolution

| What | Order |
|---|---|
| Environment of a `vaultspeed` command | `--env`, else `$TARGET_BRANCH` (set by the framework's CI to the environment branch; must name an environment, else exit 2), else `$DBT_DEPLOY_TARGET`, else `[core].default_environment` (`LDV` unless configured). Other commands skip `$TARGET_BRANCH` |
| Framework checkout | `--framework-dir` (`$VDP_VAULTSPEED_DIR`), else `[vaultspeed].project_dir` (default `.`), relative to the current directory |
| History file | `--path`, else `[vaultspeed.history].path` below the checkout |

Environment names resolve as in `vdp`: `dev` is `LDV`, so `[vaultspeed.environments.dev]` configures `LDV` (merged key by key
with an `LDV` section, the alias wins a conflict). Other names are custom environments, ordered after the built-in ones.
`$CI_COMMIT_SHA`, when set, is stamped on every ledger entry and history row as `git_sha`.

### `vaultspeed` commands

`--framework-dir <DIR>` (`VDP_VAULTSPEED_DIR`) is accepted by every `vaultspeed` command, before or after the subcommand.

**`plan` and `deploy`.** `plan` is `deploy --preview`; both take the same options:

| Option | Meaning |
|---|---|
| `--offline` | only the JSONL ledger `vaultspeed/ledger/<ENV>.jsonl`: no database, no Vault, no Airflow (dotenv and process environment are still read for `SINGLESTORE_HOST`, used in DAG dataset URIs; a placeholder otherwise). `deploy --offline` needs `--preview` (exit 2 otherwise: use `plan --offline`) |
| `--files <GENERATION>...` | only these generations, by label (the zip file name without `.zip`, e.g. `1134_FMC 1133_DDL`); all values up to the next option are labels; staged PML always runs; a label that is not staged is an error (exit 2) |
| `--preview` (alias `--dry-run`) | plan and render, change nothing: no statement runs, no ledger, `deployed/`, archive, history, DAG save or upload |
| `--explain` | print the decision of every group and the diff of every rendered DAG |
| `--report-out <FILE>` | also write the full report (the plan, in preview) as JSON to this file (relative to the current directory) |
| `--junit <FILE>` | also write JUnit XML (GitLab test reports) |

`deploy` finds the staged PML scripts and generation zips, plans each statement against the ledger, runs what is due,
records every statement, renders and publishes the DAGs of FMC generations, tidies the staged files and writes the reports.
It connects to SingleStore, and uploads DAGs over SFTP when an `AIRFLOW_URL`/`AIRFLOW_HOST` secret exists (otherwise they are
only rendered into the checkout, with a warning). `plan` still reads the ledger and the catalog from the database unless
`--offline`; it writes only the reports (the JSON report in `<checkout>/logs/`).

**`ledger`.**

| Command | Meaning |
|---|---|
| `ledger import [--offline] [--force]` | seed the ledger from the legacy `vaultspeed/deployed/{PML,DDL,ETL}` files as `imported` entries (source `legacy:deployed`); prints files and statements per artifact kind. Refused (exit 2) when the ledger already has entries unless `--force`. `--offline`: JSONL only |
| `ledger stats [--offline]` | entries and last sequence of the environment's ledger, entries by outcome and by source. Read-only |

**`rules test <FILE> [--artifact ddl|etl]`** (default `ddl`) runs a SQL file through the pipeline a deployment uses for the
kind and prints, per statement, `#<n> <kind> <target> rules: [<ids>] raw <12-char fingerprint>` and a unified diff from raw
to effective text. DDL is split, stripped of comments (unless a `-- , SHARD KEY` hint is present) and rewritten by the DDL
rules; ETL is split and rewritten by the ETL rules. It reads the file and `vaultspeed/staging/manual_changes/`; no database,
no Vault. Exit 2 when the file or a rule file cannot be read or parsed.

**`dags render <PATH> [--host <HOST>]`**: `PATH` is a generation zip, a directory or one `.py` file (only `.py` files are
rendered). Per DAG it prints `# <name> -> <published name> [<transformations>]` and a unified diff original to rendered. A DAG
that cannot be parsed or altered is reported as an error and the others still render (exit 1). `--host` (`$SINGLESTORE_HOST`,
process environment only) is the host embedded in dataset URIs; default the placeholder `singlestore-rel1<env>.invalid`
(`<env>` lower-case). Exit 2 when the path cannot be read. Nothing is published.

**`history`.** `--path <FILE>` and `--json` are accepted by both subcommands. Both read the file for the selected
environment, never create it, and exit 2 when there is no file yet.

| Command | Meaning |
|---|---|
| `history object <IDENTIFIER> [--all]` | the statements of the object that is, or was last, named `database.name`, oldest first, across renames: run time, `operation/statement_kind`, the identifier at the time, outcome, rows, duration. `--all`: every lifespan that used the name instead. Exit 1 when the name was never seen |
| `history objects [--status live\|dropped]` | the objects: id, type, identifier, first identifier, status, origin, created, ended. An empty history prints nothing (`[]` with `--json`) |

### `release serve`

Starts the release hub of `release-ui` with the VaultSpeed extension (`[release_hub.vaultspeed] enabled = false` serves the
dbt releases only). Every option defaults to its `[release_hub]` setting; it runs until stopped and exits 2 when it cannot
start (invalid configuration, `--ui-dir` without `index.html`, unresolved secrets, address in use).

| Option | Meaning |
|---|---|
| `--repo-dir <DIR>` | the clone of the dbt repository (`[release_hub].repo_dir`, `/opt/project`); the VaultSpeed clone is `[release_hub.vaultspeed].repo_dir` |
| `--bind <ADDRESS>`, `--port <PORT>` | listen address (`0.0.0.0`) and port (`8765`; `0` picks a free one) |
| `--ui-dir <DIR>` | the built frontend; without it the hub is API only |
| `--cors-origin <ORIGIN>` | allow a frontend from this origin |
| `--validate` | validate dbt releases before pushing (needs the dbt engine) |

### Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | the command ran but its work failed: statements that failed or were refused (a refused group counts as failed), a source that could not be loaded, a DAG that did not render or publish, `history object` for a name that was never seen |
| 2 | setup or usage error: bad arguments or unknown environment, unreadable or invalid configuration (unknown key, empty `rdv_order_numbers`), missing secrets, a connection that fails, `--files` label not staged, `deploy --offline` without `--preview`, `ledger import` into a non-empty ledger, no history file, a ledger that cannot be written (the run stops and writes no report) |

### Secrets

Resolved through `[vaultspeed].secrets_precedence` (the first source that knows a key wins). Vault is read only when a
`vault:` source is listed, there is no `--no-vault`, and AppRole credentials exist; a Vault failure only warns unless
`[secrets].vault_required`. `--offline` never reads Vault.

| Need | Names (first set wins; two synonyms in one source are an error) |
|---|---|
| SingleStore host | `SINGLESTORE_HOST`, `SINGLESTORE_SERVER`, `SINGLESTORE_URL` |
| SingleStore user | `SINGLESTORE_USER`, `SINGLESTORE_USERNAME` |
| SingleStore password | `SINGLESTORE_PASS`, `SINGLESTORE_PASSWORD`, `SINGLESTORE_PWD` |
| SingleStore database, port | not secrets: `[vaultspeed.environments.<ENV>].database` and `.singlestore_port`; a `SINGLESTORE_DB`/`_PORT` secret is ignored |
| Airflow host (DAG upload) | `AIRFLOW_URL`, `AIRFLOW_HOST` (scheme and `:port` allowed; unset: DAGs are only rendered locally) |
| Airflow private key | `private_key`, `AIRFLOW_PRIVATE_KEY` (PEM; written to a 0600 temp file for the run; unset: the ssh agent or the user's keys) |
| Vault AppRole role id | `KEYVAULT_ROLE_ID`, else `KEYVAULT_ROLE_ID_LDV_ABT` (`LDV`, `ABT`, `CBT`) or `KEYVAULT_ROLE_ID_CBT_PRD` (`PRD`) |
| Vault AppRole secret id | `KEYVAULT_SECRET_ID`, else `KEYVAULT_SECRET_ID_LDV_ABT` or `KEYVAULT_SECRET_ID_CBT_PRD`, as above |

The AppRole variables are read from the dotenv file or the process environment (never from Vault itself). They are the defaults
of `role_id_vars` and `secret_id_vars` in the shared `[environments.<ENV>]` section of `vdp-core`, as are the Vault URL
(`vault_url`) and the CA file (`ca_certificate`, looked up in `[vaultspeed.paths].assets_dir`); `CBT` uses the non-production
set (`_LDV_ABT`) despite the `_CBT_PRD` variable names. Without credentials Vault is skipped (a debug log line only); a
failed login only warns. With `[secrets].vault_required` both are errors (exit 2).

The SSH user is always `[vaultspeed.dags].ssh_user` (no secret overrides it). Host keys are checked strictly (`known_hosts` of
the user, or `[vaultspeed.dags].known_hosts`). Every environment variable the binary reads, in one table:
`.agents/skills/vdp-vaultspeed-framework/references/commands.md` (section "Environment variables").

### Files of a checkout

Below `[vaultspeed.paths].vaultspeed_dir` (default `vaultspeed/`) unless noted.

| Path | Role |
|---|---|
| `staging/pre_model_input/*.sql` | PML scripts; one source, runs first, every time it is staged; a completed script moves to `deployed/PML/`, one that failed or was not reached stays staged |
| `staging/release/<n>_<KIND>.zip` | generation zips; run in numeric order; a processed zip moves to `deployed/archive/` (kept staged after failures unless `archive_on_failure`) |
| `staging/manual_changes/DDL/*.json`, `.../ETL/*.json` | rewrite rules, every `*.json` in file-name order (typically `QUERY_MODIFICATIONS.json`) |
| `ledger/<ENV>.jsonl` | the JSONL ledger (a mirror of the database ledger; the ledger itself with `--offline` or `backend = "jsonl"`) |
| `deployed/{DDL,ETL,PML}/<dir>/<JOB>.sql` | readable mirror of what ran (documentation, never state); DDL appends, ETL and PML overwrite by default |
| `airflow/dags/`, `airflow/meta/` | rendered DAGs and the metadata files they read |
| `logs/<source>/deploy_errors_<yy-mm-dd@HH:MM:SS>.csv` | failed statements of a source: `JOB_NAME, TYPE, JOB_QUERY, ERROR_MESSAGE`, all quoted |
| `<checkout>/logs/vsaf_<ENV>_<run id>.json` | the full JSON report of every run (outside `vaultspeed/`) |
| `<checkout>/config/vaultspeed.json` | source list (`FMC_SOURCES[].SOURCE_NAME`) used to name the source of a DAG |
| `<checkout>/.vdp/vaultspeed-history.duckdb` | the object history (local, do not commit) |
| `<checkout>/.devcontainer/.env`, `<checkout>/assets/` | dotenv file and CA certificates |

Zip layout: `DDL` and `GENERATE_ETL`/`ETL` zips take every `.sql` in them; `COMP` takes `DDL/` and `ETL/` folders (its DAGs too
when `publish_comp_dags`); `FMC` takes top-level `.py` DAGs and `.json` metadata. A zip name is `<number>_<KIND>`.

### Reports

| Report | Where | Content |
|---|---|---|
| console | stdout | header, per source one line `executed/failed/refused/skipped by reason`, a panel per failed statement, a totals table (`applied`, `satisfied`, `failed`, `executed_groups`, `skipped_groups`, `dags`); `--explain` adds every decision and DAG diff. In a preview `applied` counts what would run |
| JSON | `<checkout>/logs/vsaf_<ENV>_<run id>.json`, plus `--report-out` | every decision, statement and DAG diff; in a preview it is the execution plan |
| CSV | `vaultspeed/logs/<source>/deploy_errors_*.csv` | failed statements per source |
| JUnit XML | `--junit FILE` | one suite per source; one test case per group (marked skipped, refused or failed), per DAG and per source-level error |

## 4. Configuration reference

Three sections belong to this workspace; keys and defaults are those of the settings structs (`deny_unknown_fields`, an empty
section is valid). Everything else (`[core]`, `[secrets]`, `[environments.*]`, `[release_hub]`, `[jira]`, `[notify]`) belongs
to `vdp-rust-utils` or `release-ui`.

Several defaults are **names from the original deployment** this code was cleaned from, kept because they are the defaults:
the `INTEL` ticket prefix, the `S2_dv_pool` Airflow pool, the `edw_*` databases and the `root_intermediate_*` CA file names
(source names such as `refkader` in the DAG examples are the same kind). Change them per site; they are not workshop names.

| Section | Struct | File |
|---|---|---|
| `[vaultspeed]` | `VaultspeedSettings` | `crates/vdp-vaultspeed/src/config/model.rs` |
| `[vaultspeed_release]` | `VsReleaseSettings` | `crates/vdp-vaultspeed-release/src/settings.rs` |
| `[release_hub.vaultspeed]` | `VaultspeedHubSettings` | `crates/vdp-vaultspeed-hub/src/settings.rs` |

### `[vaultspeed]`

| Key | Default | Meaning |
|---|---|---|
| `project_dir` | `"."` | the framework checkout, relative to the current directory; `--framework-dir` / `$VDP_VAULTSPEED_DIR` override it |
| `secrets_precedence` | `["vault:vdp/airflow", "dotenv", "vault:vdp/{env}/vaultspeed-automation-framework", "env"]` | secret sources, highest first: `vault:<path>`, `dotenv`, `env`. `{env}` is the lower-case canonical environment (`ldv`), `{env_short}` its short name (`dev`). Separate from `[secrets].precedence`, which the dbt modules use |

`[vaultspeed.paths]` (relative to the checkout):

| Key | Default | Meaning |
|---|---|---|
| `vaultspeed_dir` | `"vaultspeed"` | staging, deployed, airflow, ledger, logs |
| `fmc_sources_file` | `"config/vaultspeed.json"` | the Python framework's source list; only `FMC_SOURCES[].SOURCE_NAME` is read; a missing file means no sources |
| `dotenv_file` | `".devcontainer/.env"` | local secrets; ignored when absent |
| `assets_dir` | `"assets"` | CA certificates; a CA file name is looked up as given (absolute, or present under the checkout), else here |

`[vaultspeed.planning]`:

| Key | Default | Meaning |
|---|---|---|
| `excluded_job_substrings` | `["DDL_SCHEMAS", "DDL_INI", "DDL_CDC"]` | jobs whose name contains one are never executed (decision `excluded`) |
| `excluded_path_substrings` | `["/MIGR/"]` | files whose path, with a leading `/`, contains one are never executed |
| `standalone_drops` | `"skip"` | `"skip"` or `"execute"`: a DDL `DROP` that no `CREATE` follows. Skipping is the Python behaviour; executing would drop views nothing recreates |
| `rdv_order_numbers` | `[5]` | DDL order numbers forming the raw-vault phase, which runs last. Must not be empty |
| `protected_rebuild_order_numbers` | `[5]` | DDL order numbers whose existing tables are never dropped and re-created with a different definition (the planner refuses). Empty allows rebuilds |
| `allow_downgrades` | `false` | let an older generation overwrite what a newer one deployed (deliberate rollbacks); off adds the downgrade guard |
| `etl_layer_priority` | `["DELTA_FIREWALL_LAYER", "BUSINESS_VAULT_LAYER"]` | ETL layers that run before all other ETL, in this order |
| `catalog_probe` | `true` | ask `information_schema` before running once-only DDL the ledger does not settle, and for `vanished` objects |
| `classify_errors` | `true` | accept "already exists" style database errors as proof for the matching statement kinds |

`[vaultspeed.failure]`:

| Key | Default | Meaning |
|---|---|---|
| `pml` | `"halt-run"` | what a failing group does: `"continue"`, `"halt-source"` (skip the rest of the source) or `"halt-run"` (stop the run) |
| `ddl`, `etl` | `"continue"` | as above; a `COMP` generation uses the stricter of the two, `FMC` always continues |
| `skip_dependents` | `true` | skip groups touching an object whose CREATE failed earlier in the same source |
| `archive_on_failure` | `false` | archive a zip although part of it failed (the Python behaviour); off keeps it staged so the next run retries only what did not run |

`[vaultspeed.snapshot]` (the `deployed/` mirror): `ddl = "append"`, `etl = "overwrite"`, `pml = "overwrite"`; the values are
`"append"`, `"overwrite"` and `"disabled"`.

`[vaultspeed.ledger]`:

| Key | Default | Meaning |
|---|---|---|
| `backend` | `"database"` | `"database"`: the table `schema.table` on the SingleStore server the session connects to is the source of truth; `"jsonl"`: the JSONL file only. The table has no environment column and `schema`/`table` are not per environment: environments that share one server share one ledger unless each run gets its own `schema` (one `--config` file per environment) |
| `schema`, `table` | `"edw_meta"`, `"vsaf_ledger"` | where the table lives; identifiers are `[A-Za-z0-9_]` only; the schema must exist, the table is created (and gains the `renamed_to` column when an older table lacks it) |
| `mirror_jsonl` | `true` | also append every entry to `vaultspeed/ledger/<ENV>.jsonl` (written, never read, with the database backend) |

`[vaultspeed.history]`: `enabled = true`; `path = ".vdp/vaultspeed-history.duckdb"` (relative to the checkout; do not commit it).

`[vaultspeed.dags]`:

| Key | Default | Meaning |
|---|---|---|
| `default_args` | `email` (eight placeholder addresses `person1@example.com` ... `person8@example.com`), `email_on_failure = true`, `email_on_retry = false`, `pool = "S2_dv_pool"` | array of `{ key, value }` tables (`[[vaultspeed.dags.default_args]]`) ensured in each DAG's `default_args` dictionary. Setting the key replaces the whole list. A value needs a Python literal form; a key must not be empty |
| `mapping_outlet_prefixes` | `["hub", "lnk", "lnd", "sat", "lks", "lds", "ref"]` | mapping names with one of these prefixes get a SingleStore dataset outlet |
| `publish_comp_dags` | `false` | also publish DAGs found in `COMP` zips (the Python framework published only `FMC`) |
| `remote_dags_dir`, `remote_meta_dir` | `"/opt/apache_airflow/dags"`, `"/opt/apache_airflow/vaultspeed_mtd"` | upload targets on the Airflow host (metadata is uploaded before DAGs) |
| `ssh_user`, `ssh_port` | `"airflow"`, `22` | SSH user and port; `AIRFLOW_URL`/`AIRFLOW_HOST` may carry a `:port`, which wins |
| `known_hosts` | unset | `known_hosts` file for strict host-key checking; the user's default when unset |

`[vaultspeed.environments.<ENV>]` (one table per environment):

| Key | Default | Meaning |
|---|---|---|
| `singlestore_ca_certificate` | `LDV`, `ABT`, custom: `"root_intermediate_nonprod.cert"`; `CBT`, `PRD`: `"root_intermediate_prod.cert"` | CA file (see `paths.assets_dir`); a value means TLS with chain and host name verified, empty means no TLS |
| `database` | `"edw_dv_rdv"` | the environment's database: the connection database, the ledger's default database for unqualified names, the dataset zone in DAG URIs |
| `singlestore_port` | `3306` | connection port |
| `dataset_env` | `""` | token in `vdp://<token>/<database>/<source>` dataset URIs; empty derives it from the SingleStore host: the part from `rel1` to the next dot (`singlestore-rel1ldv.example.com` gives `rel1ldv`). A host without `rel1` and no value here stops every `plan` and `deploy` with exit 2 before anything runs (checked with `SINGLESTORE_HOST=localhost`), even when no FMC zip is staged |

Built-in environments are always present with their own defaults; a partial section keeps the rest of that environment's
defaults. `dev`, `val`, `prd` are the aliases of `LDV`, `ABT`, `PRD`; several spellings of one environment merge key by key,
an alias winning a conflict whatever its case.

### `[release_hub.vaultspeed]` and `[vaultspeed_release]`

| Key | Default | Meaning |
|---|---|---|
| `[release_hub.vaultspeed] enabled` | `true` | serve the VaultSpeed releases; `false` serves the dbt releases only |
| `repo_dir` | `"/opt/vaultspeed"` | the hub's own clone of the framework repository |
| `repo_url` | `"git@git.example.com:vdp/vaultspeed-automation-framework.git"` | its origin |
| `gitlab_url` | `""` | the GitLab for sign-in rights and pipelines; empty falls back to `[release_hub].gitlab_url` |
| `gitlab_project`, `push_url` | `""` | empty is derived from `repo_url` (`git@host:group/project.git` gives project `group/project` and push URL `https://host/group/project.git`) |
| `[vaultspeed_release] environments` | `["LDV", "ABT", "CBT", "PRD"]` | environment branches, lowest first |
| `auto_environments` | `["LDV"]` | environments CI feeds itself: every zip branch is merged automatically |
| `direct_merge_environments` | `["LDV", "ABT"]` | environments that may receive single zip branches (the others only release branches) |
| `main` | `"main"` | the neutral skeleton branch |
| `zip_prefix`, `release_prefix` | `"release/"`, `"feature/"` | zip branches (VaultSpeed's agent; never written by the tools) and release branches |
| `ticket_prefix` | `"INTEL"` | Jira project prefix read from the comment headers in zips |
| `staging_dir`, `pml_dir`, `archive_dir`, `logs_dir` | `"vaultspeed/staging/release"`, `"vaultspeed/staging/pre_model_input"`, `"vaultspeed/deployed/archive"`, `"vaultspeed/logs"` | the repository paths the flows and the status read |
| `skeleton_excluded` | `vaultspeed/staging/release/`, `.../pre_model_input/`, `vaultspeed/deployed/`, `vaultspeed/airflow/`, `vaultspeed/ledger/`, `vaultspeed/logs/`, `logs/` | path prefixes a commit on `main` may never touch |

The hub side also uses `[release_hub]` (sign-in `auth`, `keep_jobs`, `git_timeout_secs`, `conflict_timeout_secs`, `state_dir`;
the VaultSpeed job history is kept in `<state_dir>/vaultspeed`, in memory when `state_dir` is empty).

## 5. Crates and architecture

| Crate | Responsibility | Entry files |
|---|---|---|
| `vdp-vaultspeed-domain` | the pure model, no IO: generation names, jobs and order keys, statements, fingerprints, decisions, plans, run reports, `LedgerEntry`, `Lifespan`, the `ChangeRecorder` port | `src/lib.rs` |
| `vdp-vaultspeed-sql` | splitting (on `vdp-sql-script`), JSON rewrite rules, classification, fingerprints, `ArtifactPipeline` | `src/pipeline.rs`, `src/rewrite/`, `src/classify.rs` |
| `vdp-vaultspeed-dags` | in-memory Airflow DAG alteration (tree-sitter) and the publishers | `src/pipeline.rs`, `src/transform/`, `src/publish.rs` |
| `vdp-ssh` | system OpenSSH: strict host keys, atomic SFTP uploads, checked remote commands, `SshAccess::from_secrets`. It moved here from `vdp-rust-utils`, which has no use for it | `src/lib.rs` |
| `vdp-vaultspeed` | config, staged sources, planner, executor, engine, ledger (database or JSONL), history port, reports, and the wiring | `src/lib.rs`, `src/wiring.rs` |
| `vdp-vaultspeed-history` | the DuckDB object history | `src/lib.rs`, `src/schema.sql`, `src/record.rs` |
| `vdp-vaultspeed-release` | release rules, guards, zip status, plan preview, flows `build_release`, `merge_into_environment`, `commit_files`, `reset_branch` | `src/lib.rs`, `src/flows/` |
| `vdp-vaultspeed-hub` | the VaultSpeed side of the release hub: `VsHub`, `VaultspeedExtension` (segment `vaultspeed`), `VaultspeedHubSettings` | `src/extension.rs` |
| `vdp-vaultspeed-cli` | the composition root and the `vdp-vaultspeed` binary (`doc = false`) | `src/args.rs`, `src/help.rs`, `src/commands/`, `src/hub.rs` |

External crates only this workspace needs: `duckdb` (bundled), `tree-sitter` and `tree-sitter-python`, `sqlformat`,
`similar`, `zip`, `csv`. The features `ts` of `vdp-vaultspeed-hub` and `vdp-vaultspeed-release` switch on the TypeScript
bindings (`cargo test -p vdp-vaultspeed-hub --features ts export_bindings`; `VDP_TS_OUT=<file>` writes them, appended to the
hub's own types by `vdp_release_hub::typescript_with`).

```
vdp-vaultspeed-cli  composition root: args, bootstrap (secrets, session, Airflow), commands, hub::extensions
 |- vdp-vaultspeed          engine -> planner, executor, ledger, sources, report; wiring picks the implementations
 |    |- vdp-vaultspeed-sql, vdp-vaultspeed-dags, vdp-ssh          adapters on the domain's types
 |    '- vdp-vaultspeed-domain                                     pure, depends on nothing here
 |- vdp-vaultspeed-history  DuckDB adapter of ChangeRecorder (domain only)
 '- vdp-vaultspeed-hub -> vdp-vaultspeed-release -> vdp-vaultspeed (plan previews)
```

Layering: the domain does no IO; the engine depends on traits (`LedgerReader`/`LedgerWriter`, `CatalogProbe`,
`StatementExecutor`, `ErrorClassifier`, `SourceDiscovery`, `SourceHandler`, `SourceLifecycle`, `GroupingStrategy`,
`IdempotencyPolicy`, `ExecutionGuard`, `PhaseStrategy`, `DagPublisher`, `RemoteTransport`, `ReportSink`, `ChangeRecorder`);
**`vdp_vaultspeed::wiring` is the only place that picks implementations**. The CLI acquires the infrastructure (secrets,
ADBC session, Airflow transport, ledger) and calls `wiring::engine_with_history`; the hub's plan preview calls `wiring::engine`
with an offline infrastructure. Both therefore plan identically. `vdp-vaultspeed-cli` embeds `vdp-cli` and
`vdp-release-ui-cli` as libraries: it parses their global options, runs every command that is not `vaultspeed` through
`vdp_release_ui_cli::run_command`, and starts the hub through `vdp_release_ui_cli::hub::serve` with the VaultSpeed extension.

Design rules: the ledger decides what runs and the history only records; offline mode touches no Vault, database or Airflow;
a run with failures is `Outcome::Failures` (exit 1), not an error; options are named structs; secrets use a precedence chain
and `secrecy`; no unsafe code (`unsafe_code = "forbid"`), clippy `all` and `pedantic` clean.

## 6. Planning, ledger and object history

### How a run goes

1. Discover: staged PML (when at least one `.sql` is staged) and the generation zips, PML first, then generations in numeric
   order (`113` before `1134`).
2. For each source, in order: load it (a zip is read in memory, nothing is extracted), split, rewrite, classify and fingerprint
   its statements, group them, and plan them against the ledger **as it is after the previous source executed**.
3. Run the plan phase by phase (PML; DDL non-raw-vault then raw vault; ETL by layer priority), recording every statement in
   the ledger as it happens, so an interrupted run resumes correctly.
4. After each source: mirror what ran to `deployed/`, render and publish the DAGs of FMC generations (only when every DAG of
   the generation rendered and nothing halted), move the PML script or archive the zip. At the end write the reports.

### The decision of a group

The policies are asked in order, the first with an opinion wins: exclusions, staged PML (always runs), replaceable objects
(views, procedures, functions, table comments: run when the definition changed), run-once for everything else. Guards may
then veto an execute.

| Decision | Reasons |
|---|---|
| execute | `new` (never seen here), `changed` (a replaceable object whose definition differs), `always` (staged PML), `stale` (it ran, but its effect was undone since), `vanished` (the ledger says it exists, the catalog does not) |
| skip | `already applied` (ledger entry), `already in effect` (the catalog or a database error proves it), `excluded`, `upstream failed` (its table's CREATE failed earlier in the source), `halted`, `duplicate in plan` (an earlier group of the plan runs the same statements) |
| refuse | a guard vetoed it; reported as a failed group so a person looks; nothing runs and nothing is recorded |

### Planning with renames

A name is reused: `x` is created, renamed to `x_old`, and a later release creates `x` again. "Was this CREATE applied?" is then
the wrong question; the planner asks whether the object that lives under the name now started with this CREATE. It reads that
from the ledger, following renames back to the name an object was first created under. Only entries from real runs take
part in that. The rules (each pinned by a test in `crates/vdp-vaultspeed/src/planner/tests.rs`):

- A CREATE whose table was renamed away or dropped afterwards runs again (decision `stale`). The ledger entry of the earlier
  CREATE no longer describes what lives under the name.
- A rename round trip (`x` to `x_old` and back to `x`) keeps the CREATE applied: what lives under `x` is still the object that
  began with it. A seed INSERT or ADD COLUMN stays applied too, because a rename carries rows and columns along; after a drop
  and re-create they are stale and run again.
- A protected rebuild (`planning.protected_rebuild_order_numbers`, the raw vault by default) is refused when the ledger shows
  the table living and the incoming DROP + CREATE has a different definition: it would destroy the data. That includes a table
  that is there because another one was renamed to it. Tables that are gone, and new ones, are not affected.
- With the catalog probe (`planning.catalog_probe`), a table or view CREATE that the ledger says is applied, but whose object is
  not in `information_schema` any more (dropped or renamed outside the framework), runs again (decision `vanished`).
- Unqualified names resolve to the environment's database (`[vaultspeed.environments.<ENV>].database`), and after a
  `USE <database>` to that one, so `ALTER TABLE x ...` under `USE edw` is the same object as `ALTER TABLE edw.x ...`. This
  holds for the ledger, the planner and the history alike.
- Entries imported from the legacy `vaultspeed/deployed` files prove nothing about order. A name known only from them has no
  lifespan, and its CREATE is decided by its fingerprint as before.
- A downgrade (an older generation touching what a newer one last changed) is refused unless `planning.allow_downgrades`.

### Ledger and history

| | Ledger | History |
|---|---|---|
| Question | what already ran in this environment | what happened to each database object, in order |
| Role | **decides** what runs | **records**, never decides |
| Store | table `edw_meta.vsaf_ledger` on the environment's SingleStore server (source of truth; one table per server, see below) plus `vaultspeed/ledger/<ENV>.jsonl` (committed audit trail); JSONL only with `--offline` or `backend = "jsonl"` | local DuckDB file `.vdp/vaultspeed-history.duckdb`, not committed |
| Written by | every applying run (not a preview) | every applying run when `history.enabled`; a failing history warns and never stops a run |
| Entries | one per applied, satisfied or failed statement (`failed` never counts as applied) | one per statement in any outcome, including skipped and refused ones |

The ledger assigns a sequence to each entry; later entries win. `imported` entries (from `ledger import`) are not ordered.

**One ledger per server, not per environment.** A `LedgerEntry` has no environment field, the table has no environment column
(`crates/vdp-vaultspeed/src/ledger/sql.rs`), and `wiring::ledger` takes `[vaultspeed.ledger].schema` and `.table` for every
environment alike. "A ledger is bound to one environment" therefore holds only with one SingleStore server per environment
(the JSONL mirror is per environment, `ledger/<ENV>.jsonl`). Environments that share a server, as `dev`, `val` and `prd` do in
the workshop stack, would share one table: what one environment ran would count as applied in the others. Give each its own
`schema` (the schema must exist) through one `--config` file per environment. The history is not affected: its rows carry
`environment`.

### Object history

Every statement of an applying `deploy` run is stored in a local DuckDB file and linked to the database object it changes, so
the whole life of a table, view or procedure (create, alter, data changes, rename, drop) can be read back, also across
renames. A preview, an `--offline` run and a run with `[vaultspeed.history].enabled = false` record nothing. If the file cannot
be opened, the run warns and continues without a history.

The file is local to the machine and checkout that ran the deployments, and its content is what this checkout did. **Do not
commit it** (ignore `.vdp/` in the framework repository). It is created with its parent directory on the first applying run,
and `meta` holds the layout version (`schema_version`, now 1); a file written by a newer build is refused (`deploy` warns and
goes on without a history; the `history` commands exit 2). There is no migration yet.

| Table or view | Content |
|---|---|
| `statements` | One row per statement the framework handled, in any outcome. Key and links: `statement_id` (primary key, from `statement_id_seq`), `object_id` (the object it belongs to, if any), `object_type`. Target: `database`, `schema` (always equal to `database`: SingleStore has nothing between the two), `name`, `identifier` (`database.name` at that time). What: `operation`, `statement_kind`, `column_name`, `renamed_to`. Text: `raw_sql`, `effective_sql`, `formatted_sql`, `applied_rules` (list of rule ids), `raw_fp`, `effective_fp`. Result: `success` (NULL when nothing ran), `rows_affected`, `outcome` (`applied`, `satisfied`, `failed`, `skipped`, `refused`, `not_run`), `decision`, `error_code`, `error_message`, `duration_ms`. Time: `creation_time` (when this raw SQL was first seen in the environment), `run_time`, `recorded_at` (insert time). Origin: `environment`, `run_id`, `source`, `job`, `rel_path`, `group_id`, `phase`, `statement_index`, `ledger_sequence`, `git_sha` |
| `objects` | One row per object lifespan: `object_id` (primary key, from `object_id_seq`), `environment`, `object_type`, `family` (`relation` for tables and views, else the type), `database`, current `name` and `identifier`, `first_identifier`, `origin` (`created`, or `untracked` when the first statement seen was not a CREATE), `created_statement_id`, `created_at`, `status` (`live`, `dropped`), `ended_at`, `ended_statement_id`. The identity survives renames |
| `object_names` | The name history of each object: `object_id`, `database`, `name`, `identifier`, `valid_from`, `valid_to` (NULL for the current name), `from_statement_id`, `to_statement_id` |
| `meta` | `key`, `value`; `schema_version` lives here |
| `live_objects` | view: `objects` where `status = 'live'` |
| `object_lifespan` | view: every statement (all `statements` columns) joined to its object, with `object_identifier`, `object_status`, `object_origin` in front, ordered by `object_id`, `run_time`, `statement_id` |
| `rename_history` | view: successful renames (`object_id`, `environment`, `from_identifier`, `to_identifier`, `run_time`, `statement_id`) |

Only statements that took effect (`applied`, `satisfied`) change a lifespan. Statements that did not run are kept with their
outcome, so a refused or skipped group is visible too.

Open the file with the `duckdb` CLI (`duckdb .vdp/vaultspeed-history.duckdb`). DuckDB lets one process write at a time, so
read it when no deployment is running. An object's lifespan across a rename (the object that is, or was last, `edw.hub_x`; it
can have been renamed since):

```sql
SELECT run_time, operation, identifier, renamed_to, outcome, rows_affected, duration_ms
FROM object_lifespan
WHERE object_id = (
    SELECT max(object_id) FROM object_names
    WHERE identifier = 'edw.hub_x' AND object_id IN (SELECT object_id FROM objects WHERE environment = 'LDV')
)
ORDER BY run_time, statement_id;
```

The failed statements of the last run:

```sql
SELECT run_time, identifier, operation, error_code, error_message, raw_sql
FROM statements
WHERE outcome = 'failed'
  AND run_id = (SELECT run_id FROM statements ORDER BY run_time DESC LIMIT 1)
ORDER BY run_time;
```

The `history` commands (section 3) read the same file for the selected environment, without creating it: `history object
edw.hub_x` prints the statements of that object, `--all` lists every lifespan that used the name (a name is reused when a
table is renamed away or dropped and created again), `history objects` lists the objects.

## 7. VaultSpeed releases and the hub extension

**Release management has no command line.** The release flows exist only through the hub (`vdp-vaultspeed release serve`);
`vdp-vaultspeed-release` is used by `vdp-vaultspeed-hub`, and the CLI crate uses only its `VsReleaseSettings` to configure the
hub (the crate docs saying "used by the CLI" are stale).

Branch model (decisions the flows keep):

- `main` is a neutral skeleton: the framework without DAGs or SQL state; deploy state never lands there, and paths in
  `skeleton_excluded` are never touched by a commit on it.
- The environment branches `LDV`, `ABT`, `CBT`, `PRD` sit on top of `main` and carry their own deploys. `LDV` is fed
  automatically by CI; `LDV` and `ABT` also take direct merges of single zip branches. Nothing flows back or sideways: a source
  that contains an environment's own commits is refused, and nothing is based on an environment branch.
- `release/<n>_<TYPE>` is one branch per zip, created by VaultSpeed's agent and **never written** by the tools.
- `feature/<DROP>` (`2026_DROP25.1`) is the release: built from `main`, collecting zip branches and PML scripts. The `.n`
  numbers VaultSpeed releases within one DROP.
- Merges into environments use GitLab's merge message (`Merge branch '<source>' into '<ENV>'`), which CI deploys on; a direct fix
  may not look like one. A reset is a force push with the dropped commits listed.

Every write flow can run as a preview (`dry_run`), and a real run takes a lease on what the preview saw. The flows run inside a
job that was already accepted (202): a branch that moved since the preview, a refused source or a conflict that is aborted ends
that job as failed, with the message in the job (preview again), not as an HTTP error. Routes, under `/api/v1/vaultspeed/`:

| Route | Does |
|---|---|
| `GET /status[?jira=refresh]` | zips, environments, releases; 409 until the repository is cloned |
| `POST /repo/clone`, `/repo/sync`, `/repo/repair`, `/repo/reclone` | jobs on the hub's own clone |
| `GET /pipeline?sha=` | the GitLab pipeline for a commit |
| `GET /file`, `/tree`, `/history`, `/changes`, `/changes/file` | what origin has: a file, a directory, a branch's history, changed files and diffs |
| `POST /releases`, `/merges`, `/commits`, `/resets` | jobs: build a release, merge into an environment, a direct fix, reset a branch (each with a dry run) |
| `GET /jobs`, `/jobs/{id}`, `/jobs/{id}/conflict`; `POST /jobs/{id}/resolution` | the jobs, and the decision at a conflict |

Reading needs a sign-in, each write the GitLab right on the branch it changes (push, or force push for resets and recreated
releases; clone, re-clone, repair and a conflict decision need a push right on `main` or an environment; `POST /repo/sync`
needs none beyond the sign-in), and jobs run one at a time. The full model (flows, guards, rights, jobs, status codes) is in
`.agents/skills/vdp-vaultspeed-framework/references/release-model.md`. Settings: section 4.

## 8. Using it with the workshop

**Not part of the workshop, and untested against it.** What follows is what the code and the stack suggest would be needed;
nothing here was run against the workshop's SingleStore.

| Workshop fact | Consequence for `vdp-vaultspeed` |
|---|---|
| Databases carry the environment as a prefix (`dev_edw_dv_rdv`, ...) | the defaults `edw_dv_rdv` and `edw_meta.vsaf_ledger` do not exist there: set `[vaultspeed.environments.dev] database = ...` (`dev` is the alias of `LDV`) and a `[vaultspeed.ledger] schema` that exists |
| `dev`, `val` and `prd` live on one SingleStore server | the SQL ledger table has no environment column and `schema`/`table` are not per environment, so the three would share **one** ledger (section 6). Use a different `[vaultspeed.ledger] schema` per environment, which means one `--config` file per environment (the `[vaultspeed.environments.<ENV>]` tables are per environment, the ledger table is not) |
| No Vault, no TLS | `[vaultspeed] secrets_precedence = ["dotenv", "env"]` (the workshop's `[secrets] precedence` is a different setting), and `singlestore_ca_certificate = ""` (the default CA file would make the connection verify TLS) |
| SingleStore is published on host port `WORKSHOP_S2_PORT` (default 23306); inside `workshop_net` it is `singlestore:3306` | `singlestore_port` defaults to 3306, which on the host is the personal SingleStore, **not** the workshop's: set host and port explicitly and never point it at an instance that is not the workshop's |
| `profiles.yml` reads `SINGLESTORE_HOST`, `SINGLESTORE_USER`, `SINGLESTORE_PASS` | the deployer reads the same three names (plus synonyms) from the secrets |
| The workshop's SingleStore host (`singlestore`, `127.0.0.1`) has no `rel1` token | `dataset_env` is empty by default and is derived from such a host; without it every `plan` and `deploy` stops with exit 2 before anything runs. Set `dataset_env` (checked offline with `SINGLESTORE_HOST=localhost`) |
| Airflow runs in containers | `AIRFLOW_URL`/`AIRFLOW_HOST` needs an SSH server with SFTP; whether the workshop's Airflow has one is not verified. Leave the secret unset and the DAGs are only rendered into `vaultspeed/airflow/` |
| No VaultSpeed zips or PML scripts in the stack | a checkout has to be staged by hand; `rules test`, `dags render` and `plan --offline` need no service at all |

A starting point, to be adapted (untested), one file per environment (here `dev.toml`), given with `--config dev.toml`:

```toml
[vaultspeed]
secrets_precedence = ["dotenv", "env"]

[vaultspeed.environments.dev]
database = "dev_edw_dv_rdv"
singlestore_ca_certificate = ""
singlestore_port = 23306            # the workshop's published port, with SINGLESTORE_HOST pointing at the host
dataset_env = "<token>"             # the host has no `rel1` part to derive it from

[vaultspeed.ledger]
schema = "<a schema that exists, different for val.toml and prd.toml>"
```

### Hooking it up: a plan, not implemented

Status today (checked by grep over `infrastructure/` and `dbt-singlestore-workshop/`, `src/` folders excluded): no compose
service, Makefile target, CI job, nginx route, overview entry or manager step mentions this workspace, no image contains
`vdp-vaultspeed`, and `dbt-singlestore-workshop/vdp.toml` has no `[vaultspeed*]` section. Nothing below has been built or run.
First decide whether it belongs at all: the stack has no VaultSpeed zips, PML scripts or framework repository, so a service
would have nothing to deploy until someone stages content.

1. **Binary.** Either a stage in `infrastructure/dbt-singlestore-dev-image/Dockerfile` after `vdp-build` (copying its shape,
   `cargo build --release --locked -p vdp-vaultspeed-cli` in `vaultspeed-framework`, skipped with a message when the folder is
   absent, the binary installed next to `vdp` so `vdp vaultspeed ...` finds it), or a dedicated image. Both need
   `vdp-rust-utils`, `release-ui` and `vaultspeed-framework` in one build context (`src/`) and pay for compiling DuckDB and
   tree-sitter. Keep new Dockerfile lines outside the span the Studio scenario `same-toolbox` quotes, then run
   `docker exec workshop-studio python -m studio.catalog --check`.
2. **A framework checkout** with the layout of section 3 (`vaultspeed/staging/...`, `config/vaultspeed.json`), mounted or
   cloned where the process runs. The workshop's GitLab holds `dbt-vdp`; a framework repository would be a second project.
3. **One config file per environment** (`dev.toml`, `val.toml`, `prd.toml`, given with `--config`): the environment's `database`,
   `singlestore_port`, `dataset_env`, and its own `[vaultspeed.ledger] schema` (see above). A `vdp.toml` in the checkout is not
   read.
4. **Vault and TLS off:** `secrets_precedence = ["dotenv", "env"]` (or `--no-vault`) and `singlestore_ca_certificate = ""`.
5. **Secrets** `SINGLESTORE_HOST`, `SINGLESTORE_USER`, `SINGLESTORE_PASS` from the checkout's `.devcontainer/.env` (the default
   `paths.dotenv_file`) or the process environment; leave `AIRFLOW_URL`/`AIRFLOW_HOST` unset (DAGs are then only rendered).
   Which workshop user may create the ledger table and run DDL on the environment's databases is not decided here.
6. **Runner:** a compose service, a CI job or a manual step that runs `vdp-vaultspeed --config <env>.toml --env <env> vaultspeed
   plan`, then `deploy`; `release serve` with the extension would additionally need its own GitLab project for the framework
   repository (`[release_hub.vaultspeed]`).
7. **Documents to update when done** (all say "standalone, not in the stack" today): root `README.md` and `CLAUDE.md`, the
   architecture skill (section 12), `infrastructure/dbt-singlestore-dev-image/README.md`, and in this workspace `README.md`
   (sections 1 and 8), `CLAUDE.md` and the skill's "How it fits the stack".
8. **Verify:** `plan --offline` on the checkout, then `plan` and `deploy` against a scratch environment, `ledger stats`, and
   `history objects` for what the run changed.

## 9. Testing and verification

About 183 `#[test]` functions (counted with grep; 182 run without `--features ts`) and 10 doc tests: the `lib.rs` of each of
the nine crates has a runnable example, and `vdp-ssh` has a second one that is compile-only (`no_run`). Everything runs without
a database, Vault, Airflow host or GitLab.

| Crate | `#[test]`s (about) | What they cover |
|---|---|---|
| `vdp-vaultspeed-domain` | 26 | generation and job names, order keys, layers, paths, fingerprints, object references, lifespans from ledger entries |
| `vdp-vaultspeed-sql` | 25 | classification, fingerprints, the pipeline, rewrite rules, Python `re` translation |
| `vdp-vaultspeed-dags` | 6 | text edits, Python literals, DAG naming, source resolution, both publishers (the crate's one doctest runs `EnsureImport`) |
| `vdp-ssh` | 6 | secrets to endpoint, shell quoting, the SFTP batch script |
| `vdp-vaultspeed` | 80 | config (6), engine with fake executors (20), planner on a `MemoryLedger` with the real pipeline and runner (9, the rename rules), ledgers (20: memory, JSONL, a fake SQL session, composite, overlay, import), executor (6: runner, classifier, probe), reports (5), sources (11), wiring (3) |
| `vdp-vaultspeed-history` | 9 | lifespans across renames, drops, re-creations, satisfied and failed statements, reopening a file |
| `vdp-vaultspeed-release` | 7 | release names and guards, zip names and tickets, one zip change comparison |
| `vdp-vaultspeed-hub` | 5 | route status codes, result type names, the clone lifecycle on a local bare repository, and (only with `--features ts`) the TypeScript export |
| `vdp-vaultspeed-cli` | 19 | argument parsing and the help rules (8), the hub extension's GitLab resolution (1), one command (1), and `tests/process.rs` (9): the binary as a process, `--help` lists `vdp` and VaultSpeed commands, `release serve --help`, the `history` commands on a seeded file, NDJSON log, `dags render` of a missing file |

In Docker, without a Rust toolchain on the host, from the repository root:

```
make rust-check W=vaultspeed-framework       # fmt, clippy (all features, -D warnings), tests, docs
make rust-test W=vaultspeed-framework S="-p vdp-vaultspeed"   # only tests; S= goes to cargo test
make rust-check                              # all three workspaces, after a change in vdp-rust-utils or release-ui
```

The toolchain image is `workshop-rust-check` (`infrastructure/rust-check/`, `rust:1.96-trixie` with a C and C++ compiler for DuckDB and tree-sitter, the
version read from `ARG RUST_VERSION` of the dev image `Dockerfile`). It runs as the host user and mounts the whole `src/` folder, because this
workspace reaches the other two by path. The cargo registry and the build output live in Docker volumes (`make rust-clean` removes them), so no
13 GB `target/` appears next to the sources; the first run compiles DuckDB and takes long. `CARGO_BUILD_JOBS=<n>` limits the parallelism;
`make rust-cargo W=vaultspeed-framework S="<cargo arguments>"` runs any other cargo command. The TypeScript export writes a file you choose
(`VDP_TS_OUT`): run that one with a host `cargo`. With a host `cargo`, in this folder:

```
cargo fmt --check
cargo clippy --workspace --all-targets
cargo test --workspace
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
cargo test -p vdp-vaultspeed-hub --features ts export_bindings
```

This workspace depends on `vdp-rust-utils` and `release-ui`, and a change to their public API is not verified until this one
builds and passes too. `cargo check` is not enough: run `cargo build --workspace --locked` and then
`cargo test --workspace --locked` here, and, when API types of the hub changed, regenerate the TypeScript bindings
(`VDP_TS_OUT=<file> cargo test -p vdp-vaultspeed-hub --features ts export_bindings`).

Measured with `cargo test --workspace --locked` on Rust 1.96 (2026-11-20): 182 tests and 10 doc tests pass, none fail or
are ignored. That includes `wiring::tests::dataset_env_follows_the_python_convention`. Without `--features ts` the hub crate
has 4 tests (`tests/hub.rs` 3, `tests/clone_lifecycle.rs` 1).

Not covered here: the SQL ledger, the catalog probe and the executor against a real SingleStore through ADBC; Vault; the
Airflow host and SFTP; the GitLab API and Jira; the release flows, the status, the `EnvironmentCommits` guard and the plan
preview (no tests in this copy; `VsRules::guard` has one); the DAG edits themselves (default arguments, outlets); a corpus of
real VaultSpeed zips compared with the Python framework.

## 10. Known gaps

- No live-service tests (SingleStore, Vault, Airflow host, GitLab, Jira); the real ADBC session is `vdp-db` in the sibling.
- No fixture zips and no Python golden masters ship in this copy, although defaults and comments refer to the Python
  framework's behaviour; there is no parity check.
- The release flows (`build_release`, `merge_into_environment`, `commit_files`, `reset_branch`), the status, the
  `EnvironmentCommits` guard and the plan preview have no tests here (`VsRules::guard` has one); the hub tests cover route
  status codes and the clone lifecycle only.
- The DAG transformers for default arguments and outlets have no tests; the crate's only doctest runs `EnsureImport`.
- One SQL ledger per SingleStore server, not per environment (section 6): environments on one server need one `--config` file
  each with its own `[vaultspeed.ledger] schema`.
- There is no command line for releases; the hub is the only way.
- `[vaultspeed.dags].ssh_user`: the field's comment says a `username` secret overrides it; the wiring passes no user secret
  names, so the configured user is always used.
- The JSON report is written to `<checkout>/logs/` while the CSVs go to `<checkout>/vaultspeed/logs/<source>/`, and the
  `vaultspeed` help lists `logs/` among the directories below `vaultspeed/`. The CSV file names contain colons
  (`@HH:MM:SS`), which Windows file systems reject.
- The DAG upload needs the system `sftp` and a host key in `known_hosts`; it uses batch mode, so there is no password prompt.
- The history has no migration path yet (schema version 1); DuckDB allows one writing process at a time.
- A `vdp.toml` in a directory without `dbt_project.yml` is not read (checked), so a bare framework checkout needs `--config`
  or `/etc/vdp/vdp.toml` for its settings.
- The DAG context is built for every run: a SingleStore host without a `rel1` part needs `dataset_env`, or `plan` and `deploy`
  stop with exit 2 before anything runs, even when no FMC zip is staged.
- Not part of the workshop stack: section 8 is a list of mismatches, not a tested recipe.
