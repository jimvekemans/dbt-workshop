//! The long help (`--help`) of `vdp-vaultspeed`: what it adds to `vdp-release-ui`, and the `vaultspeed` commands.
//!
//! The short help (`-h`) is the one-line `about` and `help` in [`crate::args`]. The texts are wrapped by hand (clap
//! does not wrap without its `wrap_help` feature): lines of a `long_about` or `after_long_help` stay within 96
//! columns, lines of an option's `long_help` within 86 (clap indents those by 8).

pub(crate) const ROOT_LONG_ABOUT: &str = "\
The vdp toolkit with the VaultSpeed Automation Framework.

`vdp-vaultspeed` is the whole `vdp-release-ui` command line (`vdp`'s, plus `release serve`) and adds
`vaultspeed`: deployments of PML scripts and VaultSpeed generations to SingleStore and Airflow, the
ledger of what was deployed, rewrite-rule and DAG tooling, and the local object history. Here
`release serve` also serves the VaultSpeed releases. Every other command works as in `vdp`, with the
same options and help (`vdp-vaultspeed <command> --help`).";

pub(crate) const ROOT_AFTER_LONG_HELP: &str = "\
What this binary adds to `vdp` and `vdp-release-ui`:
  vaultspeed plan | deploy         show what a deployment would run, changing nothing | run it
  vaultspeed ledger import | stats seed the ledger from the legacy `deployed` files | summarise it
  vaultspeed rules test            how the pipeline splits, rewrites and classifies a SQL file
  vaultspeed dags render           the DAG diffs of an FMC zip, a directory or a .py file
  vaultspeed history object | objects  what happened to a database object | the known objects
  release serve                    the release hub of `vdp-release-ui`, plus the VaultSpeed
                                   releases (below /api/v1/vaultspeed/)

Environments (--env):
  LDV, ABT, CBT, PRD, or a name configured as [environments.<name>]. `dev`, `val` and `prd` are
  aliases of LDV, ABT and PRD (CBT has none). Without --env the environment is $DBT_DEPLOY_TARGET,
  else [core].default_environment (LDV unless configured). The `vaultspeed` commands look at
  $TARGET_BRANCH (set by the framework's CI to the environment branch) before that. An unknown name
  stops the command with exit code 2.

Configuration:
  The same vdp.toml as `vdp`, layered the same way, later files winning key by key:
    built-in defaults < /etc/vdp/vdp.toml < <dbt project>/vdp.toml < --config FILE ($VDP_CONFIG)
  The `vaultspeed` commands read [vaultspeed] (checkout, secret precedence, paths, planning,
  failure handling, ledger, history, DAGs, and per environment the database, port and certificate)
  and work on the framework checkout (--framework-dir, else [vaultspeed].project_dir). `release
  serve` reads [release_hub], [notify] and, for the VaultSpeed releases, [release_hub.vaultspeed],
  [vaultspeed_release] and [vaultspeed]. Secrets come from Vault, the dotenv file and the
  environment (--no-vault skips Vault); the `vaultspeed` commands take their order from
  [vaultspeed].secrets_precedence.

Exit codes:
  0  success
  1  the command ran, but some of its work failed: statements of a deployment that failed or were
     refused, a DAG that did not render, an object the history does not know, a failed check
  2  setup or usage error: bad arguments, unreadable configuration, unknown environment, a
     service that cannot be reached, a refused operation
  `release serve` runs until it is stopped; it exits with 2 when it cannot start.

Logging:
  Logs go to stderr: info by default, debug with -v, warnings and errors only with -q. $VDP_LOG (a
  tracing filter such as `debug`) overrides the level. --log-json FILE also writes the events as
  one JSON object per line: info and above (debug with -v), whatever -q says.

Examples:
  # What would a deployment to LDV run? Nothing is changed
  vdp-vaultspeed --env dev vaultspeed plan

  # Deploy what is staged
  vdp-vaultspeed --env dev vaultspeed deploy

  # What happened to a table, across renames
  vdp-vaultspeed --env dev vaultspeed history object edw.hub_x

  # Any command of vdp works here too
  vdp-vaultspeed --env dev env doctor";

pub(crate) const VAULTSPEED_ABOUT: &str =
    "VaultSpeed deployments: PML and generations to SingleStore and Airflow";

pub(crate) const VAULTSPEED_LONG_ABOUT: &str = "\
VaultSpeed deployments: PML scripts and VaultSpeed generations to SingleStore and Airflow.

The VaultSpeed Automation Framework's deployer. It works on a framework checkout: --framework-dir
($VDP_VAULTSPEED_DIR), else [vaultspeed].project_dir, relative to the current directory. Below its
`vaultspeed/` directory it uses `staging/pre_model_input` (PML scripts) and `staging/release`
(generation zips) as input, `staging/manual_changes/<DDL|ETL>` for rewrite rules, `ledger/` for the
JSONL ledger, `deployed/` for the mirror of what ran, `airflow/` for the rendered DAGs and `logs/`
for the reports.

The environment is --env, else $TARGET_BRANCH (set by the framework's CI to the environment
branch), else $DBT_DEPLOY_TARGET, else [core].default_environment. `dev`, `val` and `prd` are
aliases of LDV, ABT and PRD. The settings are those of [vaultspeed] (and
[vaultspeed.environments.<ENV>] for the database, port and certificate of the environment); secrets
come from [vaultspeed].secrets_precedence.

Exit codes: 0 when everything ran, 1 when the work had failures (a deployment with failed or
refused statements, a DAG that did not render, an object the history does not know), 2 on a setup
or usage error.";

pub(crate) const FRAMEWORK_DIR_LONG: &str = "\
The checkout of the vaultspeed-automation-framework repository that the `vaultspeed`
commands work on. Default: [vaultspeed].project_dir (the current directory). A
relative path is taken from the current directory.";

pub(crate) const DEPLOY_LONG_ABOUT: &str = "\
Deploy everything staged: PML first, then generations in numeric order.

For the selected environment it finds what is staged in the framework checkout (the PML scripts,
then the generation zips), plans each statement against the ledger of what has already run
([vaultspeed.planning]: exclusions, once-only DDL, downgrade and rebuild guards, the catalog probe)
and runs what is due on SingleStore, recording every statement in the ledger. A failure is handled
per [vaultspeed.failure]: by default a PML failure halts the run, while after a DDL or ETL failure
the run goes on and skips only the groups that touch an object whose CREATE failed. At the end the
reports are written.

Reads: [vaultspeed], the secrets, the staged files, the ledger. Connects to SingleStore, and
uploads the DAGs of generations to the Airflow host over SSH when the AIRFLOW_URL or AIRFLOW_HOST
secret is set (otherwise they are only rendered into the checkout, with a warning).
Changes: the SingleStore database; the ledger (a table, plus the mirror
`vaultspeed/ledger/<ENV>.jsonl`); the `vaultspeed/deployed/` mirror and the archive of processed
zips; the rendered DAGs in `vaultspeed/airflow/` and on the Airflow host; the local object history
(when [vaultspeed.history].enabled); and the reports below.
Reports: the console report, a JSON report `logs/vsaf_<ENV>_<run id>.json` in the checkout, a CSV of
the failed statements per source in `vaultspeed/logs/<source>/`, and the files of --report-out and
--junit.
--preview (alias --dry-run) plans and renders everything and changes none of the above. `plan` is
the same as `deploy --preview`.

Exit codes: 1 when the report has failures (statements that failed, or were refused); 2 on a setup
error: invalid configuration, secrets or connection, a --files generation that is not staged, or
--offline without --preview.";

pub(crate) const DEPLOY_AFTER_LONG_HELP: &str = "\
Examples:
  # Deploy what is staged to the dev environment
  vdp-vaultspeed --env dev vaultspeed deploy

  # Only two generations (staged PML always runs), with a JUnit report for GitLab
  vdp-vaultspeed --env dev vaultspeed deploy --files 1134_FMC 1133_DDL --junit report.xml

  # Preview a deployment, explaining every decision
  vdp-vaultspeed --env prd vaultspeed deploy --preview --explain

  # In CI, the environment comes from $TARGET_BRANCH
  TARGET_BRANCH=val vdp-vaultspeed vaultspeed deploy --report-out logs/run.json";

pub(crate) const PLAN_LONG_ABOUT: &str = "\
Show what `deploy` would do, changing nothing (same as `deploy --preview`).

Plans every staged source for the selected environment and prints the decision for each group of
statements: execute (new, changed, stale, vanished, ...), skip (already applied, excluded, ...) or
refuse, and why. The statements are not executed, the ledger and the `deployed/` mirror are not
written, the DAGs are only rendered (not saved or uploaded), and nothing is recorded in the object
history. The ledger and the catalog are still read from the database, unless --offline: then only
the JSONL ledger is used and neither the database, Vault nor Airflow is touched.

Reads: [vaultspeed], the secrets (not with --offline), the staged files, the ledger.
Changes: only the reports: the console report, a JSON report in `logs/` of the checkout, and the
files of --report-out and --junit.

Exit codes: 1 when the plan has failures (a group that would be refused, for example a protected
rebuild); 2 on a setup error: invalid configuration, secrets or connection, a --files generation
that is not staged.";

pub(crate) const PLAN_AFTER_LONG_HELP: &str = "\
Examples:
  # What would run on dev?
  vdp-vaultspeed --env dev vaultspeed plan

  # Every decision and DAG diff, saved as a JSON report
  vdp-vaultspeed --env dev vaultspeed plan --explain --report-out plan.json

  # Without database or Vault: from the JSONL ledger only
  vdp-vaultspeed --env dev vaultspeed plan --offline

  # One generation only
  vdp-vaultspeed --env val vaultspeed plan --files 1134_FMC";

pub(crate) const RUN_OFFLINE_LONG: &str = "\
Use only the JSONL ledger (`vaultspeed/ledger/<ENV>.jsonl`): no database, no Vault, no
Airflow. Nothing can be applied this way, so `deploy --offline` needs --preview (use
`plan --offline`).";

pub(crate) const RUN_FILES_LONG: &str = "\
Limit the generations to these, by label (the zip's file name without `.zip`), e.g.
`--files 1134_FMC 1133_DDL`; all values up to the next option are labels. The staged
PML scripts always run. A label that is not staged is an error.";

pub(crate) const RUN_PREVIEW_LONG: &str = "\
Plan and render everything without changing anything: no statement runs, the ledger
and the `deployed/` mirror are not written, DAGs are not saved or uploaded. Also
accepted as `--dry-run`.";

pub(crate) const RUN_EXPLAIN_LONG: &str = "\
Print, besides the summary, the decision for every group of statements and the diff of
every rendered DAG.";

pub(crate) const RUN_REPORT_OUT_LONG: &str = "\
Also write the full report (the plan, in preview) as JSON to this file, relative to
the current directory. A JSON report per run is always written to `logs/` of the
checkout as well.";

pub(crate) const RUN_JUNIT_LONG: &str = "\
Also write a JUnit XML report to this file, relative to the current directory, for
GitLab's test reports.";

pub(crate) const LEDGER_LONG_ABOUT: &str = "\
Ledger maintenance.

The ledger records every statement the framework ran, per environment: in a table of the
environment's database ([vaultspeed.ledger]: schema `edw_meta`, table `vsaf_ledger` by default),
mirrored to `vaultspeed/ledger/<ENV>.jsonl` in the checkout. The planner reads it to decide what
is still due.";

pub(crate) const LEDGER_IMPORT_LONG_ABOUT: &str = "\
Seed the ledger from the legacy `vaultspeed/deployed` files.

Reads the human-readable mirror the Python framework left in `vaultspeed/deployed/` and writes its
statements to the ledger of the selected environment as imported entries, so the planner does not
run them again. Entries imported this way prove nothing about order: the planner decides their
CREATEs by fingerprint. Prints, per artifact kind, the number of files and statements imported.

Reads: the `deployed/` files, [vaultspeed], the secrets. Connects to the database (not with
--offline). Changes: the ledger (database table and JSONL mirror), or only the JSONL ledger with
--offline.
A ledger that already has entries is refused unless --force.

Exit codes: a refusal (the ledger already has entries) or an error is exit code 2.";

pub(crate) const LEDGER_IMPORT_AFTER_LONG_HELP: &str = "\
Examples:
  # Seed the dev ledger once, when moving to this framework
  vdp-vaultspeed --env dev vaultspeed ledger import

  # Only the JSONL ledger of the checkout, without database or Vault
  vdp-vaultspeed --env dev vaultspeed ledger import --offline

  # Import again over an existing ledger
  vdp-vaultspeed --env dev vaultspeed ledger import --force";

pub(crate) const LEDGER_IMPORT_OFFLINE_LONG: &str = "\
Use only the JSONL ledger of the checkout: no database and no Vault. The entries go to
`vaultspeed/ledger/<ENV>.jsonl` only.";

pub(crate) const LEDGER_IMPORT_FORCE_LONG: &str = "\
Import even when the ledger already has entries. Without it that is an error, so a
second import cannot duplicate statements by accident.";

pub(crate) const LEDGER_STATS_LONG_ABOUT: &str = "\
Summarise the ledger.

Prints the number of entries of the selected environment's ledger and its last sequence number,
then the entries by outcome and by source. Read-only.

Reads: the ledger (database, or the JSONL ledger with --offline), [vaultspeed], the secrets (not
with --offline). Changes: nothing.";

pub(crate) const LEDGER_STATS_AFTER_LONG_HELP: &str = "\
Examples:
  # The dev ledger in numbers
  vdp-vaultspeed --env dev vaultspeed ledger stats

  # From the JSONL ledger of the checkout, without database or Vault
  vdp-vaultspeed --env dev vaultspeed ledger stats --offline";

pub(crate) const LEDGER_STATS_OFFLINE_LONG: &str = "\
Read only the JSONL ledger of the checkout (`vaultspeed/ledger/<ENV>.jsonl`): no
database and no Vault.";

pub(crate) const RULES_LONG_ABOUT: &str = "\
Rewrite-rule tooling.

Rewrite rules are the files in `vaultspeed/staging/manual_changes/DDL` and `.../ETL` of the
framework checkout; the deployment applies them to every statement before it runs.";

pub(crate) const RULES_TEST_LONG_ABOUT: &str = "\
Show how the pipeline splits, rewrites and classifies a SQL file.

Runs the file through the same pipeline a deployment uses for the artifact kind and prints, per
statement: its number, kind (CREATE, ALTER, ...), target object, the rules that rewrote it and the
fingerprint of the raw text, followed by a unified diff from the raw to the effective statement.
DDL is split, stripped of comments (unless they carry SHARD KEY hints) and rewritten by the DDL
rules; ETL is split and rewritten by the ETL rules.

Reads: the file, the rules of the checkout, [vaultspeed]. It does not connect to the database or
Vault, and changes nothing.

Exit codes: 2 when the file or a rule file cannot be read or parsed.";

pub(crate) const RULES_TEST_AFTER_LONG_HELP: &str = "\
Examples:
  # How is a DDL file split and rewritten?
  vdp-vaultspeed vaultspeed rules test sql/hub_x_ddl.sql

  # The same file through the ETL rules
  vdp-vaultspeed vaultspeed rules test my_load.sql --artifact etl

  # With a checkout elsewhere
  vdp-vaultspeed vaultspeed --framework-dir ../vsaf rules test ddl.sql";

pub(crate) const RULES_TEST_FILE_LONG: &str = "\
The SQL file to run through the pipeline, relative to the current directory. It may
hold several statements.";

pub(crate) const RULES_TEST_ARTIFACT_LONG: &str = "\
Which pipeline and rules to use: `ddl` (the default) or `etl`. The rules are those
below `vaultspeed/staging/manual_changes/` of the checkout.";

pub(crate) const DAGS_LONG_ABOUT: &str = "\
DAG tooling.

Airflow DAGs generated by VaultSpeed are altered on deployment (default arguments, outlets, imports,
dataset URIs). This renders them without deploying anything.";

pub(crate) const DAGS_RENDER_LONG_ABOUT: &str = "\
Render the DAGs of an FMC zip, a directory or a `.py` file and print the diffs.

Every `.py` file found is run through the DAG alteration pipeline of the deployment
([vaultspeed.dags]) for the selected environment. For each DAG it prints `# <name> -> <deployed
name> [rules applied]` and a unified diff from the original to the rendered DAG. A DAG that cannot
be parsed or altered is reported as an error, and the others are still rendered.

Reads: the zip, directory or file; [vaultspeed.dags]. Nothing is published, and it does not
connect to the database, Vault or Airflow. Changes: nothing.

Exit codes: 1 when a DAG could not be rendered; 2 when the path cannot be read.";

pub(crate) const DAGS_RENDER_AFTER_LONG_HELP: &str = "\
Examples:
  # The DAG diffs of a generation zip
  vdp-vaultspeed --env dev vaultspeed dags render vaultspeed/staging/release/1134_FMC.zip

  # A directory of DAGs
  vdp-vaultspeed --env prd vaultspeed dags render airflow/dags

  # With the SingleStore host the dataset URIs should use
  vdp-vaultspeed --env dev vaultspeed dags render dag_load.py \\
    --host singlestore-rel1ldv.example.com";

pub(crate) const DAGS_RENDER_PATH_LONG: &str = "\
A generation zip (`.zip`), a directory, or a single `.py` file, relative to the
current directory. Only the `.py` files are rendered.";

pub(crate) const DAGS_RENDER_HOST_LONG: &str = "\
The SingleStore host embedded in the dataset URIs of the rendered DAGs. Default:
$SINGLESTORE_HOST (read from the process environment only), else the placeholder
`singlestore-rel1<env>.invalid`, where `<env>` is the lower-case environment.";

pub(crate) const HISTORY_LONG_ABOUT: &str = "\
Read the local object history (what happened to a table, view or procedure, across renames).

Every statement of an applying `deploy` run is stored in a local DuckDB file and linked to the
database object it changed, so the whole life of an object (create, alter, data changes, rename,
drop) can be read back, also across renames. The file is `[vaultspeed.history].path` in the
checkout (default `.vdp/vaultspeed-history.duckdb`); a preview, an --offline run and
`[vaultspeed.history].enabled = false` record nothing. The commands read it for the selected
environment (--env) and never create it.

Exit codes: `history object` exits with 1 when the name was never seen; both exit with 2 when there
is no history file yet.";

pub(crate) const HISTORY_PATH_LONG: &str = "\
The history file to read. Default: `[vaultspeed.history].path` in the framework
checkout (--framework-dir). A relative path is taken from the current directory.";

pub(crate) const HISTORY_JSON_LONG: &str = "\
Print JSON instead of a table: the statements (`object`) or the objects (`objects`) as
an array.";

pub(crate) const HISTORY_OBJECT_LONG_ABOUT: &str = "\
The statements that changed an object, oldest first.

Takes the object that is, or was last, named `identifier`, and follows it across renames: one row
per statement with its run time, operation and statement kind, the identifier at that time, the
outcome (applied, satisfied, failed, skipped, refused, not_run), the rows affected and the
duration. With --all it lists every lifespan that used the name instead, because a name is reused
when a table is renamed away or dropped and created again.

Reads: the history file for the selected environment. Changes: nothing.

Exit codes: 1 when the name was never seen in that environment's history.";

pub(crate) const HISTORY_OBJECT_AFTER_LONG_HELP: &str = "\
Examples:
  # The life of a table, across renames
  vdp-vaultspeed --env dev vaultspeed history object edw.hub_x

  # Every object that ever used the name
  vdp-vaultspeed --env dev vaultspeed history object edw.hub_x --all

  # As JSON, from another history file
  vdp-vaultspeed --env prd vaultspeed history object edw.hub_x --json --path ci-history.duckdb";

pub(crate) const HISTORY_OBJECT_IDENTIFIER_LONG: &str = "\
The object as `database.name`, e.g. `edw.hub_x`: the object that is, or was last,
known under that name (it may have been renamed since).";

pub(crate) const HISTORY_OBJECT_ALL_LONG: &str = "\
List every lifespan that used the name (a name can be reused after a rename or a drop)
instead of the statements of one object.";

pub(crate) const HISTORY_OBJECTS_LONG_ABOUT: &str = "\
The objects of the environment.

Lists the objects in the selected environment's history: id, type, current identifier, first
identifier, status (live or dropped), origin (created, or untracked when the first statement seen
was not a CREATE), when it was created and when it ended.

Reads: the history file for the selected environment. Changes: nothing. An empty history prints
nothing (or `[]` with --json).";

pub(crate) const HISTORY_OBJECTS_AFTER_LONG_HELP: &str = "\
Examples:
  # Every object known in the dev history
  vdp-vaultspeed --env dev vaultspeed history objects

  # What was dropped
  vdp-vaultspeed --env dev vaultspeed history objects --status dropped

  # As JSON, for a script
  vdp-vaultspeed --env dev vaultspeed history objects --status live --json";

pub(crate) const HISTORY_OBJECTS_STATUS_LONG: &str = "\
Only the objects with this status: `live` or `dropped`. Default: both.";

pub(crate) const SERVE_EXTENSION_NOTE: &str = "\
In `vdp-vaultspeed` the hub also serves the VaultSpeed releases, below `/api/v1/vaultspeed/`: the
zip inventory per environment, DROP release branches of the VaultSpeed Automation Framework
repository (a clone of its own, [release_hub.vaultspeed].repo_dir, default /opt/vaultspeed) and
their merges into the environment branches. It reads [release_hub.vaultspeed] (enabled, repo_dir,
repo_url, gitlab_url, gitlab_project, push_url), [vaultspeed_release] and [vaultspeed]. Set
`[release_hub.vaultspeed] enabled = false` to serve the dbt releases only.";

pub(crate) const SERVE_AFTER_LONG_HELP: &str = "\
Examples:
  # The hub with the dbt releases and the VaultSpeed releases
  vdp-vaultspeed release serve

  # Local use: the built frontend next to the API, on loopback only
  vdp-vaultspeed --env dev release serve --bind 127.0.0.1 --port 8765 \\
    --ui-dir ../release-ui-frontend/build

  # A frontend started with `npm run dev`, on another origin
  vdp-vaultspeed release serve --cors-origin http://localhost:5173

  # Validate the dbt releases with dbt before they are pushed
  vdp-vaultspeed release serve --validate";
