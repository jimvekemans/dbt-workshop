# Modules: the crates in depth

Read when you work inside one crate and need its key types, traits, invariants and traps. Paths are relative to
`crates/`. Dependencies between crates: `vdp-core`, `vdp-secrets` and `vdp-sql-script` depend on no other `vdp-*` crate;
`vdp-db` -> secrets, sql-script; `vdp-dbt` -> core, db, secrets; `vdp-git` -> core; `vdp-jira` -> core, secrets;
`vdp-release` -> core, git, jira; `vdp-sources` -> core, db, dbt; `vdp-sql` -> core, db; `vdp-qa` -> db, dbt;
`vdp-cli` -> core, db, dbt, secrets, plus sources, git, jira, release, sql, qa behind their features (`vdp-sql-script` arrives through
`vdp-db`).

## vdp-core: environment, configuration, context

- `environment.rs`: `Environment` is a `Copy` interned name. Built-ins `LDV`, `ABT`, `CBT`, `PRD` (`Environment::ALL`),
  aliases `dev`, `val`, `prd` (`ALIASES`). `Environment::named` resolves aliases case-insensitively and accepts custom names of
  letters, digits, `_`, `-`; `short_name()` is the way back from canonical to alias; `lower()` feeds `{env}`. Custom names are
  leaked once (`Box::leak`) to stay `Copy`.
- `config.rs`: `Config` (`load`, `from_documents`, `section::<T>`, `environment`, `environment_named`, `known_environment`,
  `unknown_sections`, `origins`); `CoreSettings`, `SecretsSettings`, `EnvironmentSettings` (`non_production()`, `production()`);
  `deep_merge`. Aliases are folded into canonical section names before merging (`canonical_environment_keys`).
  Key reference: `config.md`.
- `context.rs`: `Context` plus `ContextBuilder` (`environment`, `project`, `cwd`, `in_ci`, `provide`, `provide_named`,
  `provide_value`, `build`). Services are keyed by `TypeId`, usually `Arc<dyn Trait>`, created on first `service::<T>()`, cached,
  failures included (reported once). A cycle is an error, not a deadlock (the slot lock is released while the factory runs).
  A service nobody registered is an error naming it ("not available in this build or command"). That covers a service, not a
  command: a compiled-out module's command group does not exist at all (its `Command` variants are `cfg`-gated in `args.rs`, so
  `vdp sql ...` without the feature parses as an unknown subcommand and exits 2 with clap's "unrecognized subcommand").
- `project.rs`: `ProjectLayout::discover` (walk up to `dbt_project.yml`), `at` (for `--project-dir`), `resolve`. `repo_root` is the
  nearest ancestor with a `.git`.
- `outcome.rs`: `Outcome::{Success, Failures, Exit(u8)}`; `exit_code()` is 0, 1 or the carried code.
- `batch.rs`: `BatchReport<K, S, F>` of `BatchRecord`s (`Succeeded`, `Failed`, `Skipped { reason }`); any failure maps to
  `Outcome::Failures`. Failures that stop a whole batch (authentication, setup) stay `Err` and end the command with exit 2.
- `present.rs`: plain-text `Table` and 88-column `Panel`. `milestone.rs`: structured lifecycle events on the tracing target
  `vdp::milestone`. `file_status.rs`: `FileStatus::diff` (created, changed, unchanged). `error.rs`: `CoreError`, `display_chain`
  (error plus its source chain, without repeating text).
- Invariant: no dependency on any other `vdp-*` crate.

## vdp-secrets: secrets without the process environment

- `SecretProvider` (`name`, `get`, `keys`, `layers`) with providers `EnvProvider`, `DotenvProvider`, `MapProvider`, `ChainProvider`
  (first provider that knows the key wins; `source_of`), `VaultClient` (AppRole login, KV v2 read, revokes its own token on drop,
  TLS verified against the CA, proxy variables honoured) and `VaultKvProvider`. Helpers `first_of`, `require`, `child_env`.
- Values are `SecretString`; `Debug` impls redact. `EnvProvider::keys()` is empty (children inherit the environment), so
  `child_env` hands a child only dotenv and Vault keys.
- Implementors must be `Sized` (a hidden supertrait `AsSecretProvider` backs `layers()`): wrap a trait object in a struct.
- The chain is assembled in `vdp-cli/src/bootstrap/secrets.rs` (`resolve_secrets`, `resolve_secrets_from` with `SecretSources`,
  `ResolvedSecrets { chain, vault: VaultStatus }`). `VaultStatus` is `Skipped(reason)`, `Read(paths)` or `Unavailable(reason)`.

## vdp-db and vdp-sql-script: the database port

- `SqlSession` (`execute`, `execute_counted`, `execute_with`, `query`, `query_table`) with `SqlValue` (`Null`, `Int`, `UInt`,
  `Float`, `Text`), `Row`, `ResultSet { columns, rows }`, `DbError { code, message }` (`code` is the server error number).
  Statements run in order on one connection, so `SET @x = 1; SELECT @x` works. Without parameters `query_table` accepts a script and
  returns the last result set that has columns; with parameters it runs one statement (`?` placeholders).
- `AdbcSession` is the only implementation: it loads SingleStore's ADBC driver at runtime (`DriverSource::locate(explicit,
  executables)`: the explicit file, else `lib/dbt/libadbc_driver_singlestore.so` in a directory above one of the executables (the dbt
  engine, then `vdp`), else the system search path). The URI is `mysql://host:port/db?multiStatements=true&tls=...&timeout=...`;
  user and password are driver options, never in the URI; `ssl_ca`, `ssl_cert`, `ssl_key` are options; parameters are bound as a
  one-row Arrow batch; errors carry the driver's vendor code. Scripts are split by `vdp_sql_script::RoutineAwareSplitter`.
- `ConnectionSettings` uses the dbt profile vocabulary: `TlsMode::parse` accepts dbt's `ssl_mode` aliases. `from_map` and
  `from_secrets(...).prefix(..).build()` accept case-insensitive synonyms (`host`/`server`/`url`, `user`/`username`,
  `pass`/`password`/`pwd`, `dbname`/`db`/`database`, `port`) and reject two synonyms of one setting in one layer. The `vdp` binary
  builds its settings from the dbt profile; `from_secrets` serves the sibling binaries.
- `write_csv` is RFC 4180 with a header; `NULL` is an empty cell.
- `vdp-sql-script`: `tokenize` (strings, quoted identifiers, comments), `RoutineAwareSplitter` (keeps `BEGIN ... END` and
  `DELIMITER` blocks whole), `WholeFileSplitter`, `has_sql`, trait `StatementSplitter`.

## vdp-dbt: driving dbt v2 (the wrapper)

The wrapper's first rule: every argv that dbt accepts reaches dbt with the same meaning. If in doubt, forward unchanged.

Flow of `dbt <args>` (`vdp-cli/src/commands/dbt.rs`, then `vdp_dbt::wrapper::run`):
1. `[dbt]` read from the layered files (no environment selected yet); engine located (`locate.rs`): `[dbt].binary`, else
   `$VDP_DBT_BIN`, else the first `dbt` on `PATH` whose canonical path is not the running `vdp`.
2. Grammar (`wrapper::grammar_for`): `[dbt].grammar = "embedded"` uses `crates/vdp-dbt/grammar/dbt.json`; otherwise
   `flags::cache::for_binary` (cache hit, else `flags::generate::from_binary`, else the embedded copy). Generation runs
   `dbt --version`, `dbt completions fish` and `dbt <scope> --help` for every scope, and refuses a binary whose completions lack
   `build`, `run`, `run-operation`, `parse` or `--select`. The cache file is `<cache dir>/dbt-grammar-<fnv1a of path|size|mtime>.json`,
   written through a temporary file and a rename.
3. `argv::scan` reads the argv with clap's rules: `--flag=value`, `--flag value`, `-svalue`, `-s=value`, short clusters, `--`
   terminator, negative numbers as values, non-UTF-8 tokens as positionals. `Multi` and `OptionalMulti` flags consume until the
   next `-...` token, command names included (`dbt --select a build` has no command). Unknown flags are recorded, consume nothing.
4. dbt's own `--project-dir` replaces the project of the global options, so the right `vdp.toml`, dotenv file and secrets apply.
5. `bootstrap::context` runs now: an unknown `$DBT_DEPLOY_TARGET` or an invalid `vdp.toml` stops even `dbt --version` (exit 2).
6. `wrapper::is_offline`: `--help`, `--version`, no command, or a command in `[dbt].offline_commands` runs without resolving secrets.
   Otherwise the secret chain is resolved; a failure only warns and dbt runs without the VDP secrets.
7. `guard::decide` (never changes argv): a command in `guarded_commands` needs a flag in `selector_flags` or a service user
   (`user_secret` value ends with `service_user_suffix` and does not start with `personal_user_prefix`). Refused: message on stderr,
   exit 1. `--help` and `--version` are never guarded; `--select a b build` makes `build` a selector value, so it is not a command.
8. `argv::prepare` -> `yaml_arg` (below); each change is a warning with the before and after text.
9. `SubprocessDbt::invoke` with inherited stdio. The wrapper survives SIGINT (the terminal also signals dbt) and forwards SIGTERM,
   SIGHUP and SIGQUIT. Exit code = dbt's, or 128 + signal. The child gets `[dbt].env` defaults unless already set, `VDP_ENV` (canonical
   name), `VDP_SSL_CA` (only when the CA file exists) and every key of the dotenv and Vault layers.
10. After a failed run, `missing_source_hint` reads the new bytes of `<--log-path | $DBT_LOG_PATH | <project>/logs>/dbt.log` and
    prints `hint: ... vdp sources add <source>.<table> ...` for dbt1048 errors.

`--args` and `--vars` (`argv/yaml_arg.rs`):
- `dbt_parse` copies dbt's reading (trim, strip `'`, YAML via `dbt-yaml` `=0.9.6` with `yaml_11`, JSON as fallback); the crate and
  version must stay equal to the dbt fork's, so values are read exactly as dbt reads them.
- A value dbt accepts is forwarded byte for byte. A `--vars` value dbt would reject is retried with `python_repair` (a space after
  `:` outside quotes). `run-operation --args` additionally gets dbt's own `--vars` recovery for `{days:7}`. A changed value is
  re-emitted as spaced JSON, and only after dbt's reader reads it back as the same map.
- dbt v2 reads `yes`, `on` and `007` as strings (test `reads_scalars_exactly_as_dbt_v2`): that is dbt's behaviour, kept.

Other files: `profile.rs` (`profiles_file`: `--profiles-dir`, `$DBT_PROFILES_DIR`, the project root, `~/.dbt`; renders only the selected
target with minijinja `env_var(name, default)` answered from the secret chain then the process environment, and `var(name, default)`
which gives the default, no default is an error; maps `type: singlestore` to `ConnectionSettings` with dbt's aliases, certificates
without `ssl_mode` mean `verify_ca`); `project.rs` (`DbtProject`: `profile:`, all `*-paths`, `target-path`, `manifest_path()`);
`artifacts.rs` (serde subsets of manifest v12: nodes, sources, macros, child and parent maps; unknown fields ignored, no
dependency on dbt's own crates); `missing.rs` (dbt1048 `Source 'a.b' not found` in console text, `dbt.log` and
`--log-format json`); `engine.rs` (`DbtEngine` port with `binary()` and `invoke(&Invocation) -> Completed`; modules run
`dbt parse` through `Invocation { capture: true, .. }` or `Invocation::captured`); `flags/` (`Grammar`, `Scope`, `FlagSpec`, `Arity`, `HIDDEN_ALIASES`).

## vdp-sources: dbt sources from the SingleStore catalog

- Request: `AddRequest::parse("database.table")` (split at the first dot) or the dbt1048 events of `dbt parse --log-format json`
  (`--missing`). `SourceIndex::scan` finds which YAML file under the model paths declares a source; its physical database is
  `database`, else `schema`, else the name; Jinja is `Unsupported` unless `[sources.database_prefix]` maps it
  (`SourcesSettings::physical_of`, `target_for`).
- `Catalog` port (`tables`, `columns`) over `information_schema` with bound parameters (`SqlCatalog`); columns by name or by
  `ORDINAL_POSITION`. A table the database lacks is `NotInDatabase` with `similar::closest` suggestions; nothing is written.
- Where a table goes: a hand-written file gets a text insertion (`editor::insert_table`: after the last table of the source, in the
  file's own indentation, comments, anchors and quoting untouched; the result is parsed again and must equal the old shape plus
  exactly the new table, else nothing is written; refused: flow style `tables: [...]`, missing or empty `tables:`); otherwise the
  generated file `<generated_dir>/sources_<database>.yml` (`generated::upsert`: keeps descriptions, tests and meta, appends new
  tables and columns, drops vanished columns, sorts tables by name). A table already declared by hand is `AlreadyDeclared`.
- `rewrite`: step 1 `refresh` regenerates every configured database (a database declared in a hand-written file is skipped), step 2
  the CLI runs `dbt parse`, step 3 `trim` keeps only referenced `(source, table)` pairs (`referenced_sources`: `depends_on` of nodes
  plus `source('a','b')` calls in macros whose `source.<package>.<a>.<b>` exists); a file left with nothing is deleted. If parse
  fails the regenerated files stay untrimmed.
- `tree.rs` parses YAML with `yaml-rust2` marked events (positions: 1-based lines, 0-based character columns; a mapping's own marker
  sits on its `:`, so columns come from the first key scalar). `doc.rs` is an order-preserving model whose emitter writes block style,
  lists indented under their key; scalars that were plain stay plain, new ones are quoted per YAML 1.1.
- Dependencies: it takes `SqlSession`, `DbtEngine` and `missing_sources` from `vdp-db` and `vdp-dbt`; it never names the
  subprocess or the driver.

## vdp-git, vdp-jira, vdp-release: the DROP release flow

Shared with the sibling release-ui workspace (its hub runs the same flows), so a behaviour change here changes both.

The invariant: `dev` is never written, not even locally. `BranchRules { prefix, source, main, ticket_prefix }` (`from_settings`)
validates targets (`release_target`: `<prefix>*`, not the source or main, a valid ref name) and builds the git guard
(`guard()`: `protected = [source]`, `writable = [prefix, main]`). `GitCli::with_guard` makes every writing call check
`symbolic-ref HEAD` (a detached HEAD is refused) and every push check the branch (`GitError::Guard`). Keep both layers in every new path.

- `vdp-git`: `Vcs` port (fetch, log, merge-base, `revert_with`, `cherry_pick_with`, `merge_with`, conflict resolution
  `resolve_path`/`continue_op`/`skip_op`/`abort_op`, `push(branch, PushMode)`, `diff_files`, `merge_tree`, `commit_files`, ...),
  `GitCli` over the system `git` (`-c core.hooksPath=/dev/null`, `GIT_TERMINAL_PROMPT=0`, `LC_ALL=C`; optional `with_timeout`,
  `with_identity`, `with_guard`, `with_push_target`; the CLI bootstrap sets the guard and the `[git]` timeout). `PushMode::{Normal, ForceWithLease,
  Expect(Option<sha>)}`; `Expect` leases on exactly the head a preview saw. A failed push is classified (`PushRejection`:
  non-fast-forward, stale lease, protected, hook, auth, network) and verified by a fetch. `PushTarget::Url { url, token }` passes the
  token through the child's environment (`http.extraHeader`), never in argv; `Debug` redacts. `ApplyOptions { on_conflict: Theirs |
  Pause, strategy: TheirsHunks | Strict }`: `Theirs` resolves leftover conflicts for the change being applied and names the files,
  `Pause` leaves the operation in progress as `Applied::Conflicted`.
- `vdp-jira`: `IssueTracker` port (`ticket`, `transitions`, `transition`), `JiraClient` (REST v2, basic authentication, proxy variables,
  sprint field), `FixVersion` (`^\d{4}_E?DROP\d{1,2}$`, ordered by year, then DROP before EDROP, then number; `DROP5` equals `DROP05`),
  `advance_tickets` (per-ticket errors are recorded; authentication and transport errors stop the batch).
- `vdp-release::scan`: the source's first-parent mainline since the merge-base, oldest first. A merge is a ticket merge (first ticket id
  in the message, others are `also_mentions`; whole ids), a `hotfix/` or `bugfix/` pseudo-ticket, a ticketless `branch:<name>`
  pseudo-ticket, a GitLab revert MR or a back-merge (both reported as dev violations). `revert_depth`: git's `This reverts commit`
  trailer, else the quoted depth of the subject (`Revert "` +1, `Reapply "` +2). `compare_state` gives per ticket the net state on the
  compare branch (reverted or re-enabled; the latest change wins).
- `policy.rs` `ReleasePolicy::classify`: any EDROP fix version keeps the ticket (checked first); else the latest regular DROP decides
  (after the target: revert); else a name in `[jira].excluded_fix_versions` reverts; else `NoFixVersion`, which reverts unless
  `keep_empty`.
- `analysis.rs` `Analyser { vcs, rules, tracker, ... }.analyse(&AnalysisRequest) -> Analysis` (tickets, compare reverts, uncoupled
  commits, dev violations, Jira data per ticket; a Jira error is kept per ticket in `jira_error`, and the CLI refuses to create a
  release with any). `source_head` and `compare_head` fingerprint it; flows call `check_fresh` and refuse a moved ref (`Stale`).
- `origin.rs`: `OriginState` (what a preview saw on origin; the push leases on it), `release_only` (commits only a release branch or
  main has), `check_scope`/`BranchRole` (`Newest`, `Previous`, `ReadOnly`: only the newest two release branches can change; a new
  release must be newer than the newest; a promoted branch is patched, not recreated).
- Flows: `ReleaseFlow` (branch from the analysed source head; `Revert` per selected merge, newest first; `CherryPick` of compare
  reverts of tickets still out; changelog commit last; dbt validation, then push; recreating lists `losses` that need
  `accept_losses`), `PatchFlow` (`ReEnable` reverts a revert, `Pick` cherry-picks a merge with `-m 1`; the changelog is dropped and
  rewritten on top; push with `Expect(compare_head)`), `Promotion` and `detect` (by ancestry: nothing to promote, patch, or new; new
  resets main to the merge-base and merges `--no-ff`; losses need `accept_losses`; an older release than main's needs
  `accept_rollback`; push with `Expect(main_before)`). A dirty work tree is refused; `dry_run` changes nothing.
- `conflicts.rs`: `Conflicts { options, resolve }`; with `Pause` and a resolver, a conflict becomes a `ConflictReport` (files, the
  tickets that touched them, suggestions) answered by a `Resolution` (continue, skip, abort). Without a resolver a paused conflict
  aborts the operation. The CLI uses the default (`Theirs`, `TheirsHunks`).
- `changelog.rs`: `changelog/<fix version>.md`, JSON front matter plus Markdown, commit trailer `Vdp-Changelog: <fix version>`;
  releases carry the other changelogs forward from main. `branches.rs`: `release_branches` sorts by fix version numerically.
  `changes.rs`: the file-change overview a UI shows before a promotion, release or patch (built on `Vcs::diff_files` and `commits_touching`). Cargo feature `ts` derives TypeScript bindings (`ts-rs`) for the API types.

## vdp-sql and vdp-qa

- `vdp-sql`: `quote_table` (`table` or `db.table` of `[A-Za-z0-9_$]`, backtick-quoted; anything else `InvalidName`), `read_sql_file`,
  `run_file` (the whole file through `query_table`), `export_table` (`SELECT * FROM <quoted>`), `save_csv`, `render` (text table, cells
  capped at 60 characters, `(n rows)`).
- `vdp-qa`: `orphan_tables(&Manifest)` (models materialized as `table` that no non-test node depends on; sorted),
  `view_candidates(&Manifest, &timings)` (for each view without a `t_<view>` table or incremental model and with downstream models:
  breadth-first over downstream models, following views and stopping at the first other model per branch; sums the table and
  incremental times, two decimals; sorted by total, descending, stable), `latest_timings(session, table)` (rows of the latest
  `run_started_at`), `candidates_csv` (CRLF line endings, fixed column set). `QaSettings::run_results_table_for` renders the
  placeholders.

## vdp-cli: the composition root and the library the siblings use

- `main.rs`: a file name of `dbt` or `dbt.exe` for argv[0] runs the wrapper (`commands::dbt::multicall_globals` reads
  `$VDP_PROJECT_DIR`, `$VDP_CONFIG`, `$VDP_NO_VAULT`, the last through `args::parse_no_vault` like `--no-vault`, so `vdp` and the link agree); otherwise `Cli::parse()`. `Command::External` (any subcommand clap does not
  know) goes to `external::run` with the original argv, before logging or configuration.
- `args.rs`: `Cli`, `GlobalArgs` (global flags, `env =` bindings), `Command` and the subcommand enums, each behind its feature.
  Doc comments are the short help; long help lives in `help.rs` as constants. Tests in `args.rs` enforce `about`, `long_about`, an
  `Examples:` block on every leaf, help on every argument, and lines of at most 100 columns; `help.rs` keeps `long_about` lines
  within 96 and option `long_help` within 86 (clap indents them by 8).
- `bootstrap/mod.rs`: the only place that names concrete types. `context(global, cwd)` loads configuration, selects the environment,
  warns about unclaimed sections and registers the services: `Arc<ResolvedSecrets>`, `Arc<dyn SecretProvider>`, `DbtSelection`,
  `Arc<DbtProject>`, `Arc<ResolvedTarget>` (the dbt profile), `Arc<dyn SqlSession>` (`AdbcSession`), `Arc<dyn DbtEngine>`; with
  feature `release` also `Arc<dyn Vcs>` (`GitCli` of the project's repository with the `BranchRules` guard) and
  `Arc<dyn IssueTracker>` (`JiraClient`, credentials from the secrets). A command that only needs git never logs in to Vault.
  `DbtSelection::args` appends `--profiles-dir` and `--target` so dbt reads the profile the toolkit reads.
- `commands/`: `run`, `run_command`, `dispatch` (one arm per command group; the group's function in `commands/<group>.rs`, for example
  `sql::sql`, matches the subcommand and holds the sub-arms; the logic itself lives in the module crate, here `vdp-sql`); `common.rs` has `parse` (`dbt parse`, captured),
  `print_dbt_errors`, `run_dbt_validation` (shared with the release hub). `release_present.rs` formats analyses and outcomes.
- `external.rs`: `HANDOFFS` (`vaultspeed` -> `vdp-vaultspeed`, keeps the command name; `release-ui` -> `vdp-release-ui`, drops it).
  The binary is looked up next to `vdp`, then on `PATH`; on unix `vdp` replaces itself (`exec`); not found is exit 2.
  `forwarded_args` locates the command name from the end of argv, so a global option whose value equals the command name is safe.
- `runtime.rs`: `init_logging`, `init_logging_with_capture` (stderr layer plus the optional NDJSON file layer, file mode 0600, truncated),
  `RunLogGuard` (flushes on drop), `exit_code` (prints `<program>: error: <chain>` and returns 2 for an `Err`), `run`.
  Wrapper mode (`dbt` link and `vdp dbt`) logs at `warn`, silences the `vdp::milestone` target and ignores `--log-json`.
- Public API shared with the siblings by path: `args::{GlobalArgs, Command, ReleaseCommand}`, `bootstrap::{context, adbc_driver,
  resolve_secrets_from, SecretSources, ResolvedSecrets}`, `commands::{run_command, run_dbt_validation}`, `runtime::{run, exit_code, ...}`;
  the siblings depend on `vdp-cli` with `default-features = false` and enable `sources`, `release`, `sql`, `qa` themselves.
