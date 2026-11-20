# Engine internals: planner, executor, ledger, history, DAGs, ssh

How a run works, read from the code (`crates/vdp-vaultspeed`, `-domain`, `-sql`, `-dags`, `-history`, `vdp-ssh`).
Entry files are named per topic. The planner rules also appear, user-facing, in `README.md` section 6.

## A run, in order

`Engine::run` (`crates/vdp-vaultspeed/src/engine/orchestrator.rs`):

1. `discover_sources`: PML (when at least one `.sql` is staged) and the generation zips in `staging/release`
   (optionally limited by `--files`; a label that is not staged fails). Sources are sorted by priority: PML first, then
   generations by number (`1000 + n`, numeric: `113` before `1134`).
2. For each source, in order: `prepare_source` loads it (read the zip or directory into memory, nothing is extracted;
   interpret it into statement groups and DAG files) and plans it **after the previous source executed**, so a plan
   sees everything that ran before it. A source that cannot be read or interpreted is "unloadable": reported as an
   error, and a `halt-run` policy stops the run.
3. `execute_source` runs the plan, then the `deployed/` mirror, the DAG stage (only when nothing halted), and
   housekeeping (PML script moved, zip archived or kept). It consumes the `PreparedSource`, so a plan cannot run twice.
4. The `RunReport` is handed to the report sinks (`wiring::report_sinks`).

A failing statement is data (`Outcome::Failures`, exit 1). A ledger that cannot be written is an error: the run stops
so nothing is left unrecorded, and no report is written (exit 2). A preview uses `DryRunExecutor`, an `OverlayLedger`
(later sources see what earlier ones would have done, nothing persists), no history and no `deployed/` mirror.

## Statements: split, rewrite, classify, fingerprint

`ArtifactPipeline` (`crates/vdp-vaultspeed-sql/src/pipeline.rs`): `RoutineAwareSplitter` (from `vdp-sql-script`, keeps
procedure bodies whole) then preprocessors (DDL: `StripComments::unless_shard_key_hint`, which keeps comments of a
statement that has a `-- , SHARD KEY` line because the rewrite rules turn that comment into SQL) then the rewriter (DDL
and ETL: `RuleSet` from `staging/manual_changes/<KIND>/*.json`; PML: `NoRewrite`, it is written for the platform) then
`classify_in` then fingerprints. A raw statement that starts with `DELIMITER` or `SOURCE` is rejected.

- Rule file: `{"modifications": [{"reason", "regex_pattern", "replacement", "exclusions": [...], "exclusion_groups": [...]}]}`.
  Rule id `<file stem>#<index>`; Python `re` syntax is translated (`(?m)` is always on, `\1` becomes `${1}`, `\Z`, escaped
  spaces). Exclusions are tested per match (not once per statement, as the Python framework did).
- `raw_fp` hashes the text VaultSpeed delivered, `effective_fp` the text that runs. **`raw_fp` drives decisions**, so a
  changed rewrite rule does not make everything "new"; `effective_fp` is for audit and for legacy state that kept only
  executed SQL. `normalize` drops comments and whitespace, lower-cases words and identifiers, keeps literals, strips
  trailing semicolons, and keeps hint comments (`SHARD KEY`, `SORT KEY`, keys, constraints) in normalised form, because
  they change what executes.
- Classification looks at the leading keywords: `StatementKind` (create/drop/alter with its operation, truncate, insert,
  update, delete, merge, replace, call, set, select, use, other) and the target `ObjectRef` (lower-case, backticks
  removed). Unqualified names resolve to the environment's database (`pipelines_for`), and after `USE <db>` to that one;
  fingerprints are unaffected (they hash text).
- `Statement::object_key` gives the ledger key `family:schema.name` plus a facet: definition (create, drop, rename),
  comment, one column, data, or other alteration. A rename is indexed under both names.
- Jobs and order: `JobName` drops numeric segments and upper-cases (`629_05_GENERATE_DDL_DV_x_20250114.sql` becomes
  `GENERATE_DDL_DV_X`); `OrderKey` reads up to three leading numeric segments (at most four digits, the generation
  number removed) and compares them numerically; `Layer` comes from the directory (`INCR/FOUNDATION_LAYER/`) or the job
  prefix. `DropCreatePairing` joins a `DROP x` with a later `CREATE x` into one group (a deliberate rebuild); ETL and PML
  are one group per file. A group identical to an earlier group of the same job is dropped.

## Planner

`Planner::plan` (`planner/decide.rs`) orders the groups into phases (`planner/phasing.rs`), then decides each group by
asking the policies in order, **the first with an opinion wins**; guards may then veto an `Execute`.

| Phases | Names |
|---|---|
| PML | `pml` |
| DDL | `ddl:non-rdv`, then `ddl:rdv` (the `rdv_order_numbers`, last because raw-vault objects depend on staging and metadata) |
| ETL | one `etl:<layer>` per `etl_layer_priority` entry, then `etl:remaining` |

Inside a phase groups sort by order number, job, statement rank (DROP < CREATE < ALTER < INSERT < rest), path, position.

| Policy (`planner/policy.rs`) | Decides |
|---|---|
| `ExclusionPolicy` | `excluded_job_substrings`, `excluded_path_substrings`, standalone DDL drops (when `standalone_drops = "skip"`) |
| `AlwaysRunPolicy` | PML: always `execute: always`, staging is the trigger |
| `ReplaceablePolicy` | views, procedures, functions and single-statement table comments (`GroupNature::Replaceable`): run when the definition differs from the last applied one; run again when the object this definition created is gone (`stale`) or, with the probe, missing from the catalog (`vanished`) |
| `OnceOnlyPolicy` | everything else: skip when the ledger covers every statement; with the probe, skip when the catalog proves every missing statement already holds; otherwise `execute: new` (or `stale` when everything missing ran before and was undone since) |

| Guard | Vetoes |
|---|---|
| `RebuildGuard` | a DROP + CREATE rebuild of a table of a protected order number when the ledger shows the table living and the incoming definition differs: it would destroy the data (also a table that arrived by rename). New tables and gone tables are not affected |
| `DowngradeGuard` (unless `allow_downgrades`) | executing a group that would roll an object back to an older generation than the one that last changed it (only ordered, real-run entries count; PML is exempt) |

A refusal becomes `Decision::Refuse` and is **reported as a failed group** (exit 1) so a person looks; nothing runs and
nothing is recorded. After the chain, a run-once group whose every statement an earlier group of the plan already executes
is skipped as `duplicate in plan`. Decision texts: `Decision`'s `Display` in `vdp-vaultspeed-domain/src/decision.rs`.

Lifespan logic (`planner/lifespan.rs`, `staleness.rs`, domain `lifespan.rs`): `current_lifespan` walks the ledger history
of a definition key backwards over **ordered** entries (real runs; `imported` and `failed` entries are not ordered). A
DROP or rename-away ends the name, a CREATE or rename-into starts it, and a rename's origin is the origin of the object
under its old name before the rename. `Standing` means the object living under the name began with this very CREATE;
`Ended` means the CREATE ran but its object is gone. `superseded_since` finds later changes that undid an applied
statement: a CREATE, DROP or RENAME of the table voids its columns, comment and rows (unless the lifespan shows the same
object, as after a rename round trip); a column `ADD`/`CHANGE` is undone by a `DROP COLUMN` and the reverse, `MODIFY`
by any later change to the column; rows only by `TRUNCATE` or replacing the table.

## Executor

`PlanRunner::run` (`executor/runner.rs`), per group, in plan order:

- `skip_dependents`: an `Execute` group that touches an object whose CREATE failed earlier in the source becomes
  `skip: creation of <object> failed earlier in this run`.
- `Execute`: statements run in order through the `StatementExecutor` (`SessionExecutor` on the ADBC session,
  `DryRunExecutor` in preview). After a failed statement the rest of the group is `not_run`. Every applied, satisfied or
  failed statement is **recorded in the ledger immediately** (an interrupted run resumes correctly); `not_run`, skipped
  and refused statements are not. A ledger write error stops the run.
- `Skip(Satisfied)`: the catalog proved the effect already holds; every statement is recorded as `satisfied`.
- Failure policy per source (`engine/failure.rs`): `Continue`, `HaltSource`, `HaltRun`; `COMP` takes the stricter of
  ddl and etl, `FMC` always continues. After a halt every later group is `skip: run halted`.
- `ErrorClassifier` (`MySqlErrorClassifier`, off with `classify_errors = false`): a database error becomes `satisfied`
  evidence only for the matching kind: 1060 on `ADD COLUMN`, 1091 on `DROP COLUMN`, 1051 on `DROP TABLE|VIEW`,
  1008 on `DROP DATABASE`, 1007 on `CREATE DATABASE`, 1050 on a plain `CREATE TABLE`. Anything else is a failure.
- `InformationSchemaProbe` (`executor/probe.rs`, built only with a session and `catalog_probe`): `exists` for tables and
  views (`vanished`), and `verify` for a plain `CREATE TABLE` (table present), `ADD COLUMN` and `MODIFY COLUMN` (column
  present with a compatible type; a different type is `Contradicts`, which refuses the group), `DROP COLUMN` (table
  present, column absent), `RENAME` (source gone and target present holds; both present contradicts). A DROP + CREATE
  group never uses the probe: it is a deliberate rebuild.
- Every statement of every group, whatever its outcome, is also reported to the `ChangeRecorder` (the history). A
  failing recorder is logged and never fails the run.

## Ledger

Traits in `ledger/traits.rs`: `LedgerReader` (`find_applied` by raw then effective fingerprint, `latest_for_object`,
`object_history`, `last_sequence`, `entries`), `LedgerWriter` (`record` assigns the sequence), `LedgerSink` (append of an
already sequenced entry). A ledger is bound to one environment.

| Implementation | Role |
|---|---|
| `MemoryLedger` | indexes over successful entries; the base of all others and the test double. Failed entries are kept but never count as applied. Unqualified object keys of older entries are qualified with the environment's database **in memory only** |
| `JsonlLedger` | append-only file, one `LedgerEntry` JSON per line (`renamed_to` omitted when empty); `snapshot_in` reads without creating anything (preview); a bad line is `Corrupt` |
| `SqlLedger` | table `schema.table` on the server the session connects to (no environment column: one ledger per server, see below), loaded into memory at open; writes go to the table first. `CREATE ROWSTORE TABLE IF NOT EXISTS` with `sequence BIGINT UNSIGNED PRIMARY KEY`, `run_id`, `recorded_at DATETIME(6)`, `source`, `job`, `rel_path`, `object_key`, `statement_kind`, `raw_fp CHAR(64)`, `effective_fp CHAR(64)`, `outcome`, `detail`, `git_sha`, `renamed_to`; adds `renamed_to` to an older table; a snapshot never creates or alters |
| `CompositeLedger` | reads and sequences in the primary, appends to mirrors; a failing mirror is logged, not fatal |
| `OverlayLedger` | buffers writes in memory over a read-only base (preview) |
| `LegacyImporter` | reads the Python framework's `deployed/{PML,DDL,ETL}/**.sql` and records each statement as `imported` (source `legacy:deployed`, effective fingerprint only) |

`wiring::ledger`: backend `database` with a session is a `CompositeLedger` (table primary, JSONL mirror when
`mirror_jsonl`); no session or backend `jsonl` is the `JsonlLedger`. In preview both are read-only snapshots. **With the
database backend the JSONL file is written and never read.** The table is the same for every environment (`LedgerSettings`
has one `schema` and one `table`, `LedgerEntry` no environment), so "a ledger is bound to one environment" holds only with
one SingleStore server per environment; environments on one server need a different `schema` each, i.e. one `--config` file
per environment. `LedgerOutcome`: `applied`, `satisfied`, `imported`, `failed`
(`counts_as_applied` is everything but `failed`). Imported entries prove nothing about order (the Python framework kept
one file per job).

## Object history

Port `ChangeRecorder` (domain `change.rs`); adapter `DuckDbHistory` (`crates/vdp-vaultspeed-history`). One connection
behind a mutex, one transaction per event. `ChangeEvent` carries the statement, raw and effective SQL, fingerprints,
applied rules, decision text, outcome (`applied`, `satisfied`, `failed`, `skipped`, `refused`, `not_run`), rows, error,
ledger sequence, timing, `git_sha`. Only `applied` and `satisfied` events change an object's lifespan (`record.rs`):

- `CREATE` over a live object: `OR REPLACE`, `IF NOT EXISTS` and replaceable types (view, procedure, function) continue
  the object; a plain `CREATE` over a live table means it was dropped outside the framework, so the old lifespan ends and
  a new one starts. `DROP` ends the live object (an unknown object gets an `untracked` lifespan born and ended).
- Other statements link to the live object, starting an `untracked` one when none is live. A rename closes the old
  name, opens the new one, and ends any live object already under the target name.
- A `satisfied` statement never starts a lifespan by itself: it confirms what lives there (a satisfied CREATE of an
  unknown table is `untracked`, a satisfied DROP of a live object ends it).
- `creation_time` of a statement is the first `run_time` the same `raw_fp` was seen in that environment.

Schema: `crates/vdp-vaultspeed-history/src/schema.sql`; `SCHEMA_VERSION = 1` in the `meta` table. Opening a file written
by a newer build fails (`SchemaTooNew`); `deploy` warns and continues without a history, the `history` commands exit 2.
There is no migration yet: older versions would be migrated step by step in `DuckDbHistory::init` before the idempotent
schema pass. A new column or table needs a `SCHEMA_VERSION` bump and that migration.

## DAGs

`DagStage` (`engine/dag_stage.rs`) handles the kinds in `publish_kinds` (`FMC`, plus `COMP` with `publish_comp_dags`).
Per DAG: parse with tree-sitter (`DagDocument`), apply the transformers in order, validate, name, diff. The pipeline
(`wiring::dag_pipeline`): one `default-args:<key>` per `default_args` entry, `import:airflow.datasets.Dataset`,
`outlets:mapping-tasks`, `outlets:load-success`; then `SyntaxValidator`. Transformers return span-based `TextEdit`s, are
idempotent (no edits when the change is present) and never touch Python with regular expressions; the document is
re-parsed after each. `outlets:load-success` needs the DAG's source system: a configured source from
`config/vaultspeed.json` that is a token of the file name, else the `<gen>_FL_DAG_<source>_...` convention, else the DAG
fails (`UnknownSource`). The published name drops numeric segments and upper-cases (`VaultSpeedDagNamer`):
`1134_FL_DAG_refkader_incr_20251023_104445.py` (`refkader` is a source name from the original deployment) becomes
`FL_DAG_REFKADER_INCR.py`, so each generation replaces the
previous one.

**Nothing is published unless every DAG rendered.** Publishers (`CompositePublisher`, stops at the first failure):
`RepoDirPublisher` writes `vaultspeed/airflow/{dags,meta}` (temp file and rename), then `SftpPublisher` uploads to
`remote_meta_dir` and `remote_dags_dir` when an Airflow host secret exists. Metadata goes before DAGs because Airflow
parses a DAG as soon as it appears. A preview renders only.

## `vdp-ssh`

System OpenSSH client, no library. `SshAccess::from_secrets(secrets, SecretNames)` returns `None` without a host secret,
strips a scheme and reads a `:port`, writes the private key to a 0600 temp file that lives as long as the access.
`OpenSshTransport` runs `sftp -b <batch>` with `BatchMode=yes` and `StrictHostKeyChecking=yes` (an unknown host key fails
the upload; `[vaultspeed.dags].known_hosts` or the user's file must know it), uploading each file under a temporary name
and renaming it into place; a non-zero exit aborts the batch. `OpenSshShell` runs checked remote commands.
Oddity: `wiring::airflow_secret_names` passes no user secret names, so the SSH user is always `dags.ssh_user`.

## Wiring

`crates/vdp-vaultspeed/src/wiring.rs` is the only composition of implementations: settings to `Planner`, handlers,
pipelines, ledger, executor, classifier, probe, discoveries, lifecycles, DAG stage and report sinks, assembled in
`engine_with_history`. The CLI acquires the infrastructure (`vdp-vaultspeed-cli/src/bootstrap.rs`: secrets, ADBC
session, Airflow transport, ledger) and hands it over as `Infrastructure`; the hub's `plan_preview` builds an offline
`Infrastructure` and calls `wiring::engine`. Both therefore plan the same way. A new implementation is picked here and
nowhere else. `engine_with_history` always builds the DAG stage, whose `dag_context` needs `dataset_env` or a SingleStore
host containing `rel1`: otherwise every run fails in wiring (exit 2) before any statement runs.
