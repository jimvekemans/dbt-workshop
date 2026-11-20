---
name: vdp-vaultspeed-framework
description: >-
  The `vaultspeed-framework` Cargo workspace (infrastructure/dbt-singlestore-dev-image/src/vaultspeed-framework): the
  VaultSpeed Automation Framework's deployer as the binary `vdp-vaultspeed` (crates vdp-vaultspeed, -domain, -sql, -dags,
  -history, -release, -hub, -cli, and vdp-ssh). Covers `vaultspeed plan` and `deploy` of PML scripts and generation zips
  (DDL, ETL, COMP, FMC) to SingleStore and Airflow, the planner and its rename-aware rules, the ledger (database table or
  JSONL), the DuckDB object history and its `history` commands, rewrite rules, DAG alteration and SFTP publication,
  reports (JSON, CSV, JUnit), the VaultSpeed release model (main skeleton, environment branches, zip branches, DROP
  releases) and its hub extension below /api/v1/vaultspeed, the `[vaultspeed]`, `[vaultspeed_release]` and
  `[release_hub.vaultspeed]` settings, and how the workspace builds and fits (or does not fit) the workshop stack. Use it
  when touching crates/vdp-vaultspeed*, crates/vdp-ssh, those settings, the `vdp vaultspeed` hand-off, or when someone
  asks what this code base is and whether the workshop uses it.
---

# vaultspeed-framework

## 1. When to use

Reach for this skill for anything inside the `vaultspeed-framework` workspace or about VaultSpeed deployments and
releases. Stack-wide work starts at `vdab-dbt-workshop-architecture` (section 12 lists the three Rust workspaces). The
sibling workspaces have their own skills: `vdp-rust-utils` (the `vdp` toolkit) and `release-ui` (the hub).

## 2. Map

Workspace root: `infrastructure/dbt-singlestore-dev-image/src/vaultspeed-framework/` (`README.md` is the long reference).

| Crate | Responsibility | Start at |
|---|---|---|
| `vdp-vaultspeed-domain` | pure model, no IO: generations, jobs, statements, fingerprints, decisions, plans, reports, `LedgerEntry`, `Lifespan`, the `ChangeRecorder` port | `src/lib.rs` |
| `vdp-vaultspeed-sql` | split, rewrite rules (JSON), classify, fingerprint, `ArtifactPipeline` | `src/pipeline.rs` |
| `vdp-vaultspeed-dags` | tree-sitter DAG alteration, `DagPublisher`s | `src/pipeline.rs`, `src/publish.rs` |
| `vdp-ssh` | system OpenSSH: strict host keys, atomic SFTP, checked commands, `SshAccess::from_secrets` | `src/lib.rs` |
| `vdp-vaultspeed` | config, sources, planner, executor, engine, ledger, history port, reports, **wiring** | `src/lib.rs`, `src/wiring.rs` |
| `vdp-vaultspeed-history` | DuckDB object history (schema version 1) | `src/lib.rs`, `src/schema.sql` |
| `vdp-vaultspeed-release` | release model: rules, guards, status, plan preview, flows (build, merge, commit, reset) | `src/lib.rs`, `src/flows/` |
| `vdp-vaultspeed-hub` | the hub extension: `VsHub`, routes, `VaultspeedHubSettings` | `src/extension.rs` |
| `vdp-vaultspeed-cli` | composition root, the `vdp-vaultspeed` binary (`doc = false`) | `src/args.rs`, `src/help.rs`, `src/commands/` |

Depth, one file each:
- [references/engine.md](references/engine.md): read when changing the planner, executor, ledger, history, DAG stage or ssh, or when a decision or refusal needs explaining.
- [references/commands.md](references/commands.md): read when adding or changing a command, flag, exit code, environment variable or report, or when a file's place in the checkout matters.
- [references/config.md](references/config.md): read when adding or changing a settings key, or when a configuration error needs explaining.
- [references/release-model.md](references/release-model.md): read when touching branches, guards, release flows, hub routes, rights or jobs.

## 3. How it fits the stack

- **Standalone, not part of the workshop.** No VaultSpeed content in `dbt-singlestore-workshop/`, no `[vaultspeed*]` in its
  `vdp.toml`, no compose, CI, nginx or overview hook and no stack Makefile target (only the Rust checks, `make rust-check
  W=vaultspeed-framework`, see section 6), and no image contains the binary (the dev image builds only `-p vdp-cli` from
  `vdp-rust-utils`).
- **Build by hand:** `cargo build --release --locked -p vdp-vaultspeed-cli` in the workspace folder. The first build compiles
  DuckDB (C++) and tree-sitter (C) and takes long; `target/` measured about 13 GB after a debug plus a test build (the debug
  binary alone is 1.1 GB; a release build is smaller but slow). Rust 1.96 (`rust-toolchain.toml`).
- **Run:** `vdp-vaultspeed ...`, or `vdp vaultspeed ...` (the hand-off table `HANDOFFS` in
  `vdp-rust-utils/crates/vdp-cli/src/external.rs` keeps the command name; the binary is found next to `vdp` or on `PATH`),
  or `release serve` for the hub with the VaultSpeed extension. The framework's own CI calls it with `TARGET_BRANCH`,
  `CI_COMMIT_SHA` and `--junit`.
- **Siblings by path dependency:** `../vdp-rust-utils` (`vdp-core`, `-secrets`, `-db`, `-sql-script`, `-git`, `-jira`,
  `-release`, `-cli`) and `../release-ui` (`vdp-release-hub`, `-notify`, `-release-ui-cli`); the three folders stay side by
  side. A change to a sibling's public API is not verified until this workspace builds and passes too (section 6).
- **If someone tried it in the workshop** (untested): defaults `edw_dv_rdv` and `edw_meta.vsaf_ledger` versus prefixed
  databases (`dev_edw_dv_rdv`), needing `[vaultspeed.environments.dev]` (alias of `LDV`); no Vault and no TLS in the workshop
  (`secrets_precedence` without `vault:`, `singlestore_ca_certificate = ""`); `singlestore_port` defaults to 3306, the personal
  SingleStore's port, which is not the workshop's; a host without a `rel1` part needs `dataset_env` or every run stops with
  exit 2; and `dev`, `val`, `prd` on one server would share **one** SQL ledger (no environment column, one `schema`/`table`
  for all), so each needs its own `[vaultspeed.ledger] schema` through a `--config` file per environment (README section 8,
  which also has a numbered hookup plan, not implemented).

## 4. Rules that are easy to break

1. **The ledger decides what runs; the history only records.** The planner reads the ledger through `LedgerReader`; nothing
   reads the history to decide, and a failing recorder is logged, never fatal.
2. **Offline touches nothing**: no Vault, database or Airflow. A preview writes only reports: no ledger, `deployed/`, archive,
   history or DAG upload.
3. **`wiring` is the single place that picks implementations.** CLI and hub both build engines there, so they plan alike.
4. **`raw_fp` drives decisions, `effective_fp` audits.** Changing `normalize`, the splitter or classification silently
   re-decides every existing ledger. Treat it as a breaking change.
5. **A refusal is a failed group**; a run with failures is `Outcome::Failures` (exit 1), never an `Err`. Exit codes: 0 success,
   1 work failed, 2 setup or usage error. A ledger that cannot be written stops the run (exit 2).
6. **Branch model** (decisions, `references/release-model.md`): `main` is a neutral skeleton, never deploy state; environment branches
   `LDV`, `ABT`, `CBT`, `PRD` sit on top of it, `LDV` is auto-fed, `LDV` and `ABT` take direct merges; nothing flows back or
   sideways (a source holding an environment's own commits is refused); `feature/<DROP>` is the release; merges into
   environments carry GitLab's merge message, which CI deploys on; a reset is a force push that lists what it drops.
7. **`release/*` zip branches are never written.** Writes go through `VsRules::guard()` (`vdp_git::WriteGuard`, the mechanism
   `vdp-rust-utils` uses for the dbt branches).
8. **Release management has no CLI**; it exists only through the hub. Do not add release flows to `vaultspeed` commands
   without a decision.
9. **Help text is tested**: every command has `about`, `long_about` and an `Examples:` block, every argument has help, lines stay
   within 100 columns (96 in `long_about`, 86 in an option's `long_help`). Edit `help.rs`.
10. **Options are named structs** (`RunOptions`, `EngineInputs`, `LedgerOptions`, `ReportSinkOptions`, ...), secrets go through
    the precedence chain and `secrecy`, and unknown config keys are errors (`deny_unknown_fields`).
11. **No code is copied between the three workspaces**; reuse goes through the path dependency.
12. The DuckDB history is local and not committed; the JSONL ledger mirror is committed in the framework repository.

## 5. Playbooks

**A vaultspeed command.** Variant in `VaultspeedCommand` (`args.rs`, doc comment as `about`, `long_help` from consts); texts
in `help.rs` (`*_LONG_ABOUT` with Reads/Changes/Exit codes, `*_AFTER_LONG_HELP` with `Examples:`); an arm in
`commands/vaultspeed.rs::run` that plans or runs only through `wiring`. Return `Outcome::Failures` for failed work, `Err`
for setup errors. Update `references/commands.md` and the README tables. `args.rs` tests check the help rules.

**A rewrite or classification rule.** Rewrite rules are data in the checkout (`staging/manual_changes/{DDL,ETL}/*.json`):
try them with `vaultspeed rules test`. Code lives in `vdp-vaultspeed-sql` (`rewrite/`, `classify.rs`, `fingerprint.rs`). A new
statement shape needs a `StatementKind` or `AlterOp` variant and its `label()` and `object_key` facet in the domain, the
history's `operation()` (`record.rs`) and, for undo semantics, `staleness::undoes`. Extend the tests in
`crates/vdp-vaultspeed-sql/src/` (`classify.rs`, `fingerprint.rs`, `pipeline.rs`, `rewrite/`) and, when decisions change,
`crates/vdp-vaultspeed/src/planner/tests.rs` (real pipeline and runner into a `MemoryLedger`). **There is no ledger
migration:** existing ledgers keep the fingerprints and object keys they recorded, so a change to `normalize`, the splitter or
classification re-decides every statement against every existing ledger. Treat it as a breaking change and settle how
existing ledgers are handled before it merges.

**A planner rule or decision.** A policy (`IdempotencyPolicy`) or guard (`ExecutionGuard`) in `planner/`, registered in
`wiring::planner` (order matters, the first opinion wins, guards only veto `Execute`); new reasons in domain `decision.rs`
with `Display`, then the console `skip_label` and JUnit. Read the ledger only through `LedgerReader`. A switch becomes a key
in `PlanningSettings` (field, `Default`, test, `references/config.md`). Test in `planner/tests.rs` with `MemoryLedger`.

**A ledger or history field.** Ledger: add the field to `LedgerEntry` with `#[serde(default, skip_serializing_if = ...)]` so old
JSONL lines parse; add the SQL column like `renamed_to` (create, `ALTER` for older tables in `SqlLedger::open_in`, a snapshot
reads it as NULL and never alters); the importer leaves it empty. History: `ChangeEvent`, `schema.sql`, `record.rs`, `read.rs`,
the CLI tables; bump `SCHEMA_VERSION` and write the migration in `DuckDbHistory::init` (there is none yet); a file written by
a newer build must stay refused (`SchemaTooNew`). `history_tables_match_golden_output` in `tests/process.rs` pins the output.

**A hub route.** An arm in `vdp-vaultspeed-hub/src/extension.rs::handle`; request and response types in `api.rs` (requests with
`deny_unknown_fields`); logic on `VsHub` (`hub/mod.rs` reads, `hub/jobs.rs` writes as jobs); writes call `actor_for` with
`Need::Push` or `Need::Force`; reads and pre-job checks map errors through `hub_error`, while a flow's own failures end a job as
failed (no HTTP status); add the types to `ts.rs::declarations()` (the generated TypeScript goes to a separate frontend
repository, see `../release-ui/README.md`); pin status codes in `tests/hub.rs`; update **both** route tables: README section 7
and `references/release-model.md` (and its rights table).

**A crate.** `crates/<name>/Cargo.toml` with the `*.workspace = true` package keys and `[lints] workspace = true`; a path entry
in the root `[workspace.dependencies]`, used with `.workspace = true`; a `lib.rs` with `//!` responsibilities and a runnable
`# Example` (doctests and `cargo doc -D warnings` run on it). Pure code depends only on the domain. Commit `Cargo.lock`.

**TypeScript bindings.** `VDP_TS_OUT=<file> cargo test -p vdp-vaultspeed-hub --features ts export_bindings`; every exported type
is listed in `crates/vdp-vaultspeed-hub/src/ts.rs`. The output belongs in the separate frontend repository
(`../release-ui/README.md`); nothing here builds or tests that frontend.

## 6. Verify

Prefer the Docker targets of the root `Makefile`: the host needs no Rust toolchain, and the registry and the (very large) build output live in Docker
volumes, not in `target/`. From the repository root (long on a cold cache because of DuckDB and tree-sitter):

```
make rust-check W=vaultspeed-framework                       # fmt, clippy (all features, -D warnings), tests, docs
make rust-test W=vaultspeed-framework S="-p vdp-vaultspeed"  # a quick loop: only tests (S= goes to cargo test)
make rust-check                                              # after a change in vdp-rust-utils or release-ui: all three workspaces
```

`make rust-cargo W=vaultspeed-framework S="<cargo arguments>"` runs any other cargo command in the same container (`CARGO_BUILD_JOBS=<n>` limits
the parallelism; `make rust-clean` removes the volumes). The TypeScript export writes a file you choose (`VDP_TS_OUT`): run that one with a host
`cargo`. With a host `cargo`, from the workspace folder (long on a cold cache because of DuckDB and tree-sitter):

```
cargo fmt --check
cargo clippy --workspace --all-targets          # all + pedantic, unsafe_code forbidden; keep it clean
cargo test --workspace                          # about 183 tests (182 without `--features ts`) and 10 doc tests
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
cargo test -p vdp-vaultspeed-hub --features ts export_bindings
```

After touching `vdp-rust-utils` or `release-ui` public API, `cargo check` here is not enough: run
`cargo build --workspace --locked`, then `cargo test --workspace --locked`, and, when API types of the hub changed, the
TypeScript regeneration above. Nothing here needs a database, Vault, Airflow or GitLab; the SQL ledger, the probe and the
runner are tested against fakes. Offline smoke runs need no service and no `--no-vault`: `rules test`, `dags render`,
`plan --offline` in a throwaway checkout (`references/commands.md`).

## 7. Known gaps

No tests against a live SingleStore, Vault, Airflow host or GitLab. No fixture zips or Python golden masters ship here.
The release flows, the status, the `EnvironmentCommits` guard and the plan preview have no tests in this copy (`VsRules::guard`
has one); the DAG edits (default args, outlets) none, the crate's only doctest runs `EnsureImport`. No CLI for releases. One
SQL ledger per server, not per environment. The DAG upload needs the system `sftp` and a known host key. The `ssh_user`
field comment promises a `username` secret override that the wiring does not implement. README section 10 has the full list.
