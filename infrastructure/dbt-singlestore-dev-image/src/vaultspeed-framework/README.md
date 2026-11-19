# vaultspeed-framework

The VaultSpeed Automation Framework's deployer, as a Rust workspace next to `vdp-rust-utils`. It deploys the
PML scripts and the VaultSpeed generations staged in a framework checkout to SingleStore and Airflow, keeps a ledger
of what it ran, and keeps a local history of every database object it changed.

One binary, `vdp-vaultspeed`: the whole `vdp` command line (dbt wrapper, sources, releases, ...) plus the VaultSpeed
commands, and `release serve` (the release UI's hub) with the VaultSpeed release extension. `vdp vaultspeed ...` runs this
binary (see `vdp-rust-utils/README.md`).

```
vdp-vaultspeed --env LDV vaultspeed plan                    # what a deploy would run, changing nothing
vdp-vaultspeed --env LDV vaultspeed deploy                  # deploy staged PML and generations to SingleStore and Airflow
vdp-vaultspeed vaultspeed ledger import | stats             # seed the ledger from legacy `vaultspeed/deployed`, or summarise it
vdp-vaultspeed vaultspeed rules test file.sql               # how the pipeline splits, rewrites and classifies a SQL file
vdp-vaultspeed vaultspeed dags render fmc.zip               # the DAG diffs of an FMC zip, a directory or a .py file
vdp-vaultspeed --env LDV vaultspeed history object edw.hub_x
vdp-vaultspeed --env LDV vaultspeed history objects --status dropped
vdp-vaultspeed release serve                                # the release hub, with the VaultSpeed releases
```

The environment is `--env`, else `$TARGET_BRANCH` (set by the framework's CI for the environment branch) for the
`vaultspeed` commands, else `$DBT_DEPLOY_TARGET`, else `[core].default_environment`. The framework checkout is
`--framework-dir` (`$VDP_VAULTSPEED_DIR`), else `[vaultspeed].project_dir`, relative to the current directory.
Environment names are resolved as in `vdp`: `dev`, `val` and `prd` are aliases of `LDV`, `ABT` and `PRD`, so a
`[vaultspeed.environments.dev]` section configures `LDV` (merged key by key with a `LDV` section, the alias winning a
conflict). Any other name is a custom environment, ordered after the built-in ones.

## Build

```
cargo build --release --locked -p vdp-vaultspeed-cli      # target/release/vdp-vaultspeed
```

The workspace imports the `vdp-*` toolkit crates by path (`../vdp-rust-utils/crates/<name>`) and the release hub
(`vdp-release-hub`, `vdp-notify`) and the `release serve` command (`vdp-release-ui-cli`) from the `release-ui` workspace
(`../release-ui/crates/<name>`), so the three folders must sit next to each other. The dependencies run
`vdp-rust-utils` <- `release-ui` <- `vaultspeed-framework`. The external dependencies have the versions of `vdp-rust-utils`, and `Cargo.lock` was
started from its lock file. DuckDB is compiled from source (`bundled`, C++) and tree-sitter compiles C, so the first
build takes a while. Rust 1.96 (`rust-toolchain.toml`). Put `vdp-vaultspeed` next to `vdp` or on `PATH`.

## Crates

| Crate | Content |
|---|---|
| `vdp-vaultspeed-domain` | the pure model: generations, statements, decisions, plans, reports, ledger entries, lifespans, change events |
| `vdp-vaultspeed-sql` | lexing, routine-aware splitting, rewrite rules, classification and fingerprints (on `vdp-sql-script`) |
| `vdp-vaultspeed-dags` | in-memory Airflow DAG alteration (tree-sitter) and publication |
| `vdp-ssh` | remote access over the system OpenSSH client (strict host keys, atomic SFTP uploads, checked remote commands); the DAG publication uses it. It moved here from `vdp-rust-utils`, which has no use for it |
| `vdp-vaultspeed` | staged sources, ledger, planner, executor, engine, reports and their wiring |
| `vdp-vaultspeed-history` | the DuckDB object history |
| `vdp-vaultspeed-release` | VaultSpeed releases: the zip inventory per environment, DROP release branches, merges into environments, the guards that keep an environment's content in it |
| `vdp-vaultspeed-hub` | the VaultSpeed side of the release hub, a `vdp-release-hub` extension below `/api/v1/vaultspeed/` |
| `vdp-vaultspeed-cli` | the composition root and the `vdp-vaultspeed` binary |

`vdp-vaultspeed-cli` embeds `vdp-cli` and `vdp-release-ui-cli` as libraries: it parses `vdp_cli::args::GlobalArgs` and its
own command enum (`vaultspeed`, or any command of `vdp-release-ui`: `vdp`'s and `release serve`), runs every other
command through `vdp_release_ui_cli::run_command`, and starts the release hub through `vdp_release_ui_cli::hub::serve`
with the VaultSpeed extension (`[release_hub.vaultspeed]`, `[vaultspeed_release]`, `[vaultspeed]`). The hub itself, with
its TypeScript bindings (`VDP_TS_OUT=<file> cargo test -p vdp-release-hub --features ts export_bindings`), is in
`release-ui`; the VaultSpeed types are exported here with `cargo test -p vdp-vaultspeed-hub --features ts export_bindings`.

## Configuration

The same `vdp.toml` as `vdp`, layered the same way. The framework reads `[vaultspeed]` (checkout, secret precedence,
paths, planning, failure handling, ledger, history, DAGs, and per environment the database, port and certificate),
`[vaultspeed_release]` and `[release_hub.vaultspeed]`. Vault (URL, AppRole, CA, mount) is shared with `vdp`; the
secret precedence is `[vaultspeed].secrets_precedence`, with the framework's own dotenv file and CA folder.

## Object history

Every statement of an applying `deploy` run is stored in a local DuckDB file and linked to the database object it
changes, so the whole life of a table, view or procedure (create, alter, data changes, rename, drop) can be read back,
also across renames. The history records; it never decides what runs (the ledger does). A preview, an `--offline` run
and a run with `[vaultspeed.history].enabled = false` record nothing. If the file cannot be opened, the run warns and
continues without a history.

```toml
[vaultspeed.history]
enabled = true
path = ".vdp/vaultspeed-history.duckdb"   # relative to the framework checkout
```

The file is local to the machine and checkout that ran the deployments, and its content is what this checkout did.
**Do not commit it** (ignore `.vdp/` in the framework repository). It is created with its parent directory on the first
applying run, and `meta` holds the layout version; a file written by a newer build is refused.

### Tables and views

| | Content |
|---|---|
| `statements` | One row per statement the framework handled, in any outcome: `identifier` (`database.name` at that time), `operation`, `statement_kind`, `column_name`, `renamed_to`, `raw_sql`, `effective_sql`, `formatted_sql`, `creation_time`, `run_time`, `success` (NULL when nothing ran), `rows_affected`, `outcome` (`applied`, `satisfied`, `failed`, `skipped`, `refused`, `not_run`), `decision`, `error_code`, `error_message`, `duration_ms`, fingerprints, and where it came from (`environment`, `run_id`, `source`, `job`, `rel_path`, `group_id`, `phase`, `git_sha`, `ledger_sequence`). `schema` always equals `database`: SingleStore has nothing between the two |
| `objects` | One row per object lifespan: `object_id`, `environment`, `object_type`, `database`, current `name` and `identifier`, `first_identifier`, `origin` (`created`, or `untracked` when the first statement seen was not a CREATE), `created_at`, `status` (`live`, `dropped`), `ended_at`. The identity survives renames |
| `object_names` | The name history of each object: `identifier`, `valid_from`, `valid_to` (NULL for the current name) |
| `live_objects` | `objects` where `status = 'live'` |
| `object_lifespan` | Every statement joined to its object, ordered by object, `run_time`, statement |
| `rename_history` | Successful renames: `from_identifier`, `to_identifier`, `run_time` |

Only statements that took effect (`applied`, `satisfied`) change a lifespan. Statements that did not run are kept with
their outcome, so a refused or skipped group is visible too.

### Queries

Open the file with the `duckdb` CLI (`duckdb .vdp/vaultspeed-history.duckdb`). DuckDB lets one process write at a
time, so read it when no deployment is running.

An object's lifespan across a rename (the object that is, or was last, `edw.hub_x`; it can have been renamed since):

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

### The `history` commands

Both read the file for the selected environment's name (`--env`), without creating it, and take `--json` and
`--path` (a history file other than the configured one; `--framework-dir` changes where the configured one is).

```
vdp-vaultspeed --env LDV vaultspeed history object edw.hub_x [--all] [--json] [--path file.duckdb]
vdp-vaultspeed --env LDV vaultspeed history objects [--status live|dropped] [--json] [--path file.duckdb]
```

- `history object <identifier>` prints the statements of the object that is, or was last, named `<identifier>`
  (`database.name`), oldest first, following its renames: `run_time`, `operation/statement_kind`, the identifier at the
  time, `outcome`, `rows`, `duration`. With `--all` it lists every lifespan that used the name instead (a name is
  reused when a table is renamed away or dropped and created again). Exit code 1 when the name was never seen.
- `history objects` lists the objects (id, type, identifier, first identifier, status, origin, created, ended).

## Planning with renames

A name is reused: `x` is created, renamed to `x_old`, and a later release creates `x` again. "Was this CREATE applied?"
is then the wrong question; the planner asks whether the object that lives under the name now started with this CREATE.
It reads that from the ledger, following renames back to the name an object was first created under. The rules:

- A CREATE whose table was renamed away or dropped afterwards runs again (decision `stale`). The ledger entry of the
  earlier CREATE no longer describes what lives under the name.
- A rename round trip (`x` to `x_old` and back to `x`) keeps the CREATE applied: what lives under `x` is still the
  object that began with it.
- A protected rebuild (`planning.protected_rebuild_order_numbers`, the raw vault by default) of a table that is there
  because another one was renamed to it is refused: that table holds the other one's data, and a DROP and CREATE with
  a different definition would destroy it. Tables that are gone, and new ones, are not affected.
- With the catalog probe (`[vaultspeed.planning].catalog_probe`), a table or view CREATE that the ledger says is applied,
  but whose object is not in `information_schema` any more (dropped or renamed outside the framework), runs again
  (decision `vanished`).
- Unqualified names resolve to the environment's database (`[vaultspeed.environments.<ENV>].database`), and after a
  `USE <database>` to that one, so `ALTER TABLE x ...` under `USE edw` is the same object as `ALTER TABLE edw.x ...`. This
  holds for the ledger, the planner and the history alike.
- Entries imported from the legacy `vaultspeed/deployed` files prove nothing about order. A name known only from them
  has no lifespan, and the CREATE is decided by its fingerprint as before.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets` (pedantic, clean), `cargo test --workspace`,
  `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`.
- Known failing test: `vdp-vaultspeed wiring::tests::dataset_env_follows_the_python_convention`.
- The framework's golden, decision, edge-case, runner and scenario tests are in the `vdp-vaultspeed*` crates; the
  history's in `vdp-vaultspeed-history` (lifespans across renames, drops and re-creations, every outcome).
- Process tests of the binary (`crates/vdp-vaultspeed-cli/tests/process.rs`): `--help` lists the `vdp` and the
  VaultSpeed commands; `release serve --help` and `release analyse --help` work; `vaultspeed history` reads a file seeded through `DuckDbHistory`.
- VaultSpeed releases (`vdp-vaultspeed-release`, `vdp-vaultspeed-hub/tests/hub.rs`): on a framework repository shaped
  like the real one, the zip status per environment, building and extending a release with a lease, merges with CI's
  message, the guards (environment content never leaves its environment, `main` stays the skeleton, zip branches are
  never written), conflicts, resets with their drop list, the offline plan preview, and rights per branch from a stub
  GitLab.

Not exercised here: the history written by a real deployment against SingleStore, Vault, the Airflow host and the
GitLab API.
