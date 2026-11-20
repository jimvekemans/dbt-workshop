# Configuration: the full key tree

Three sections of the `vdp.toml` family belong to this workspace. Keys and defaults are those of the settings structs
(`#[serde(deny_unknown_fields, default)]`): an empty section is valid, an unknown key is an error (exit 2, the key is
named). This file is written once; `README.md` section 4 carries the same tables.

| Section | Struct | File | Read by |
|---|---|---|---|
| `[vaultspeed]` | `VaultspeedSettings` | `crates/vdp-vaultspeed/src/config/model.rs` | every `vaultspeed` command, and the hub (plan previews) |
| `[vaultspeed_release]` | `VsReleaseSettings` | `crates/vdp-vaultspeed-release/src/settings.rs` | the hub's VaultSpeed side (`release serve`) |
| `[release_hub.vaultspeed]` | `VaultspeedHubSettings` | `crates/vdp-vaultspeed-hub/src/settings.rs` | `release serve` (`crates/vdp-vaultspeed-cli/src/hub.rs`) |

Everything else (`[core]`, `[secrets]`, `[environments.*]`, `[release_hub]`, `[release]`, `[jira]`, `[notify]`) belongs
to `vdp-rust-utils` or `release-ui`. Layers and where a file is found: `commands.md`.

Several defaults are names from the original deployment this code was cleaned from, kept because they are the defaults: the
`INTEL` ticket prefix, the `S2_dv_pool` pool, the `edw_*` databases, the `root_intermediate_*` CA file names (and source
names such as `refkader` in examples). Change them per site; they are not workshop names.

Changing a key means three edits: the struct field, its `Default` impl, and the tests in the same file
(`empty_section_gives_python_parity_defaults` compares an empty section with `Default`). `validate()` rejects values the
planner cannot work with (today: empty `planning.rdv_order_numbers`, a `dags.default_args` entry with an empty key).

## `[vaultspeed]`

| Key | Default | Meaning |
|---|---|---|
| `project_dir` | `"."` | the framework checkout, relative to the current directory; `--framework-dir` / `$VDP_VAULTSPEED_DIR` override it |
| `secrets_precedence` | `["vault:vdp/airflow", "dotenv", "vault:vdp/{env}/vaultspeed-automation-framework", "env"]` | secret sources, highest first: `vault:<path>`, `dotenv`, `env`. `{env}` is the lower-case canonical environment (`ldv`), `{env_short}` its short name (`dev`). Separate from `[secrets].precedence`, which the dbt modules use |

### `[vaultspeed.paths]` (relative to the checkout)

| Key | Default | Meaning |
|---|---|---|
| `vaultspeed_dir` | `"vaultspeed"` | staging, deployed, airflow, ledger, logs |
| `fmc_sources_file` | `"config/vaultspeed.json"` | the Python framework's source list; only `FMC_SOURCES[].SOURCE_NAME` is read; a missing file means no sources |
| `dotenv_file` | `".devcontainer/.env"` | local secrets; ignored when absent |
| `assets_dir` | `"assets"` | CA certificates (Vault and SingleStore); a CA file name is looked up as given (absolute, or present under the checkout), else here |

### `[vaultspeed.planning]`

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
| `classify_errors` | `true` | accept "already exists" style database errors as proof for the matching statement kinds (error codes in `engine.md`) |

### `[vaultspeed.failure]`

| Key | Default | Meaning |
|---|---|---|
| `pml` | `"halt-run"` | what a failing group does: `"continue"`, `"halt-source"` (skip the rest of the source) or `"halt-run"` (stop the run) |
| `ddl`, `etl` | `"continue"` | as above; a `COMP` generation uses the stricter of the two, `FMC` always continues |
| `skip_dependents` | `true` | skip groups touching an object whose CREATE failed earlier in the same source |
| `archive_on_failure` | `false` | archive a zip although part of it failed (the Python behaviour); off keeps it staged so the next run retries only what did not run |

### `[vaultspeed.snapshot]` (the `deployed/` mirror)

| Key | Default | Meaning |
|---|---|---|
| `ddl` | `"append"` | `"append"`, `"overwrite"` or `"disabled"`: how `deployed/<KIND>/<dir>/<JOB>.sql` is kept; the mirror is documentation, never state |
| `etl`, `pml` | `"overwrite"` | as above |

### `[vaultspeed.ledger]`

| Key | Default | Meaning |
|---|---|---|
| `backend` | `"database"` | `"database"`: the table `schema.table` on the SingleStore server the session connects to is the source of truth; `"jsonl"`: the JSONL file only |
| `schema`, `table` | `"edw_meta"`, `"vsaf_ledger"` | where the table lives; identifiers are `[A-Za-z0-9_]` only; the schema must exist, the table is created (and gains the `renamed_to` column when an older table lacks it). **Not per environment**: the table has no environment column (`ledger/sql.rs`) and `wiring::ledger` uses these two for every environment, so environments on one server share one ledger. Give each its own `schema` through a `--config` file per environment |
| `mirror_jsonl` | `true` | also append every entry to `vaultspeed/ledger/<ENV>.jsonl` (written, never read, with the database backend) |

### `[vaultspeed.history]`

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | record every statement of an applying run in the local DuckDB file |
| `path` | `".vdp/vaultspeed-history.duckdb"` | relative to the checkout; do not commit it |

### `[vaultspeed.dags]`

| Key | Default | Meaning |
|---|---|---|
| `default_args` | `email` (eight placeholder addresses `person1@example.com` ... `person8@example.com`), `email_on_failure = true`, `email_on_retry = false`, `pool = "S2_dv_pool"` | array of `{ key, value }` tables (`[[vaultspeed.dags.default_args]]`) ensured in each DAG's `default_args` dictionary. Setting the key replaces the whole list. A value needs a Python literal form; a key must not be empty |
| `mapping_outlet_prefixes` | `["hub", "lnk", "lnd", "sat", "lks", "lds", "ref"]` | mapping names with one of these prefixes get a SingleStore dataset outlet |
| `publish_comp_dags` | `false` | also publish DAGs found in `COMP` zips (the Python framework published only `FMC`) |
| `remote_dags_dir`, `remote_meta_dir` | `"/opt/apache_airflow/dags"`, `"/opt/apache_airflow/vaultspeed_mtd"` | upload targets on the Airflow host (metadata is uploaded before DAGs) |
| `ssh_user`, `ssh_port` | `"airflow"`, `22` | SSH user and port; `AIRFLOW_URL`/`AIRFLOW_HOST` may carry a `:port`, which wins. The user is always `ssh_user` (the field's comment mentions a `username` secret override that the wiring does not implement) |
| `known_hosts` | unset | `known_hosts` file for strict host-key checking; the user's default when unset |

### `[vaultspeed.environments.<ENV>]` (one table per environment)

| Key | Default | Meaning |
|---|---|---|
| `singlestore_ca_certificate` | `LDV`, `ABT`, custom: `"root_intermediate_nonprod.cert"`; `CBT`, `PRD`: `"root_intermediate_prod.cert"` | CA file (see `paths.assets_dir`); a value means TLS with chain and host name verified, empty means no TLS |
| `database` | `"edw_dv_rdv"` | the environment's database: the connection database, the ledger's default database for unqualified names, the dataset zone in DAG URIs |
| `singlestore_port` | `3306` | connection port (a `SINGLESTORE_PORT` secret is ignored) |
| `dataset_env` | `""` | token in `vdp://<token>/<database>/<source>` dataset URIs; empty derives it from the SingleStore host: the part from `rel1` to the next dot (`singlestore-rel1ldv.example.com` gives `rel1ldv`). A host without `rel1` and no value here stops every `plan` and `deploy` with exit 2 before anything runs (checked with `SINGLESTORE_HOST=localhost`; `wiring::dag_context` is built even when no FMC zip is staged) |

Built-in environments (`LDV`, `ABT`, `CBT`, `PRD`) are always present with their own defaults; a partial section keeps
the rest of that environment's defaults (a `CBT` section that sets only `database` keeps `CBT`'s certificate). Names go
through the same resolution as everywhere: `dev`, `val`, `prd` are the aliases of `LDV`, `ABT`, `PRD`, several
spellings of one environment merge key by key, the canonical spelling first and any alias after it, so an alias wins a
conflict whatever its case. Any other valid name is a custom environment, ordered after the built-ins.

## `[release_hub.vaultspeed]` (the VaultSpeed side of the hub)

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `true` | serve the VaultSpeed releases; `false` serves the dbt releases only |
| `repo_dir` | `"/opt/vaultspeed"` | the hub's own clone of the framework repository |
| `repo_url` | `"git@git.example.com:vdp/vaultspeed-automation-framework.git"` | its origin |
| `gitlab_url` | `""` | the GitLab for sign-in rights and pipelines; empty falls back to `[release_hub].gitlab_url` |
| `gitlab_project`, `push_url` | `""` | empty is derived from `repo_url` (`git@host:group/project.git` gives project `group/project` and push URL `https://host/group/project.git`) |

Settings that come from `[release_hub]`: sign-in (`auth = "gitlab"` or `"none"`; the VaultSpeed side resolves its GitLab
only when sign-in is on), `keep_jobs`, `git_timeout_secs` (0: none), `conflict_timeout_secs` and `state_dir`. Job history of
the VaultSpeed side is kept in `<state_dir>/vaultspeed`; an empty `state_dir` keeps it in memory. An unknown
`[release_hub.<segment>]` table is warned about, not ignored.

## `[vaultspeed_release]` (the branches of the framework repository)

| Key | Default | Meaning |
|---|---|---|
| `environments` | `["LDV", "ABT", "CBT", "PRD"]` | environment branches, lowest first |
| `auto_environments` | `["LDV"]` | environments CI feeds itself: every zip branch is merged automatically |
| `direct_merge_environments` | `["LDV", "ABT"]` | environments that may receive single zip branches (the others only release branches) |
| `main` | `"main"` | the neutral skeleton branch |
| `zip_prefix` | `"release/"` | branches VaultSpeed's agent creates, one per zip; never written by the tools |
| `release_prefix` | `"feature/"` | release branches (`feature/2026_DROP25.1`) |
| `ticket_prefix` | `"INTEL"` | Jira project prefix read from the comment headers in zips |
| `staging_dir`, `pml_dir`, `archive_dir`, `logs_dir` | `"vaultspeed/staging/release"`, `"vaultspeed/staging/pre_model_input"`, `"vaultspeed/deployed/archive"`, `"vaultspeed/logs"` | the repository paths the flows and the status read (`logs_dir` is where the status looks for `deploy_errors_*.csv`) |
| `skeleton_excluded` | `vaultspeed/staging/release/`, `.../pre_model_input/`, `vaultspeed/deployed/`, `vaultspeed/airflow/`, `vaultspeed/ledger/`, `vaultspeed/logs/`, `logs/` | path prefixes a commit on `main` may never touch (the rewrite rules in `vaultspeed/staging/manual_changes` are skeleton and not listed) |

Model and rules: `release-model.md`.
