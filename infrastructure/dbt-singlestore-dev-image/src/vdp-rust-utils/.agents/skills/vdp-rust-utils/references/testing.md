# Testing

Read when you add or change a test, run the quality gate, or need to know what this tree does and does not verify. Counts are
from `cargo test --workspace --locked` on this tree (default features, debug profile): 282 tests in all, 0 ignored.

## The gate

In Docker, from the repository root, no Rust toolchain on the host: `make rust-check W=vdp-rust-utils` runs every command below (`rust-test`
for the tests alone; `infrastructure/rust-check/check.sh` is the list). Or run them in
`infrastructure/dbt-singlestore-dev-image/src/vdp-rust-utils` with the pinned toolchain (`rust-toolchain.toml`, 1.96).
Use bash for any shell loop over cargo flags (zsh does not word-split `$f`).

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings     # all + pedantic, from [workspace.lints]
cargo test --workspace --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --all-features
# every module feature alone must compile and pass clippy (the default build has all four):
cargo clippy -p vdp-cli --all-targets --no-default-features -- -D warnings
for f in sources release sql qa; do cargo clippy -p vdp-cli --all-targets --no-default-features --features "$f" -- -D warnings; done
```

- `--all-features` on the workspace also enables `vdp-release/ts` (TypeScript bindings of the API types through `ts-rs`, used by
  the release-ui sibling). `cargo test --workspace` runs with default features only, so after touching the API types of
  `vdp-release` also run `cargo test -p vdp-release --features ts`; no test target in the default run covers that feature.
- Clap doc comments become help text and rustdoc: wrap `[section]` and `<placeholder>` in backticks.
- `unsafe_code` is forbidden by the workspace lints; `unreachable_pub` and `missing_debug_implementations` warn; `missing_errors_doc`
  warns, so every public `Result` function has a `# Errors` section.
- The two sibling workspaces consume this workspace by path. After a change to a public item run the siblings' builds too (see
  `image-and-consumers.md`); the image build does not exercise them.

## Test layers and what exists

| Layer | Where | What it covers |
|---|---|---|
| Unit tests | `#[cfg(test)]` in `crates/*/src` | parsers, rules, guards, settings, scan, policy, YAML editing, secrets chain |
| Process tests | `crates/vdp-cli/tests/*.rs` | the built `vdp` as a child process (`CARGO_BIN_EXE_vdp`): exit codes, stdout and stderr |
| Real git | `crates/vdp-git/tests/repo.rs` | `GitCli` against throw-away repositories |
| HTTP stand-ins | `crates/vdp-jira/tests/http_contract.rs`, tests in `vdp-secrets` and `vdp-cli` | a local `tiny_http` server in place of Jira and Vault |
| Doc tests | the crate-level `# Example` in each crate's `lib.rs` (12), plus `vdp-git` and `vdp-sources` (second example, `no_run`), `vdp_secrets::SecretProvider`, and `vdp-core` (`batch.rs` 3, `present.rs` 2) | public API examples; the ones that touch git or a database are `no_run` (`vdp-db`, `vdp-git`, `vdp-sources`) |

Counts per target (`cargo test` output):

| Crate | Unit | Integration | Doc |
|---|---:|---|---:|
| `vdp-cli` | 27 | `config_reference` 3, `doctor` 1, `dry_run` 2, `environment_aliases` 3, `external` 10, `git_timeout` 2, `jira_advance` 3, `log_json` 11, `no_script_names` 2, `no_vault` 4 | 1 |
| `vdp-core` | 35 | none | 6 |
| `vdp-db` | 14 | none | 1 |
| `vdp-dbt` | 29 | none | 1 |
| `vdp-git` | 22 | `repo` 9 | 2 |
| `vdp-jira` | 13 | `http_contract` 11 | 1 |
| `vdp-qa` | 4 | none | 1 |
| `vdp-release` | 21 | none | 1 |
| `vdp-secrets` | 9 | none | 2 |
| `vdp-sources` | 15 | none | 2 |
| `vdp-sql` | 2 | none | 1 |
| `vdp-sql-script` | 10 | none | 1 |
| Total | 201 | 61 | 20 |

What the process tests pin:
- `doctor.rs`: `env doctor` honours `$VDP_ADBC_DRIVER`. `dry_run.rs`: `sql run --dry-run` prints the exact preview and writes nothing; a
  missing file is exit 2 with one message. `environment_aliases.rs`: with a workshop-shaped `vdp.toml` (`dev`, `val`, `prd`) the aliases select
  `LDV`, `ABT`, `PRD`, an unknown environment lists the four built-ins once, and `{env_short}` renders the run-results table.
- `external.rs`: with the built `vdp` copied into a directory of its own and `PATH` set to a directory the test fills, `vdp vaultspeed ...`
  and `vdp release-ui ...` pass the original arguments (global options first included, the command name dropped for `release-ui`) and
  the exit code of a stand-in binary; a missing binary is exit 2 with an explanation; another unknown subcommand is clap's error.
- `config_reference.rs`: the key tables of the README and `references/config.md` are equal and match the `*Settings` structs (sections, type
  names, keys, defaults) and `commands::MODULE_SECTIONS`; compiled-out modules (`--no-default-features`) are skipped by feature.
- `no_script_names.rs`: no Rust source (help texts, module docs, comments) names a Python script of the old utilities by its file name.
- `jira_advance.rs`: `jira advance` against a local fake Jira: output order, per-ticket failures, exit codes.
- `git_timeout.rs`: with `[git] timeout_secs = 1` and a `git` on `PATH` that never finishes, `vdp release analyse` stops it and exits 2; an unknown `[git]` key
  is refused by name.
- `no_vault.rs`: `$VDP_NO_VAULT` is one boolean for `vdp` (seen in `vdp env doctor`'s vault line) and for the `dbt` link (through a symlink to `vdp` and a
  stand-in engine): the on and off words, an empty value is off, anything else is exit 2.
- `log_json.rs`: the NDJSON run log (`--log-json`, `$VDP_LOG_JSON`): valid events, levels per flag and `$VDP_LOG`, streaming to disk
  during the run, an unwritable path warns once and keeps the exit code, `dbt` multicall writes no file. A shell-script `git` on
  `PATH` stands in for git.
- `vdp-git/tests/repo.rs`: parsers on real git output (renames, binary files, spaces, unicode), conflict detection and resolution for
  merge, cherry-pick and revert, `conflict_versions`.
- `vdp-jira/tests/http_contract.rs`: 401 and 403 are fatal authentication errors, 404 and 500 are per-ticket request errors, a refused
  connection is a fatal transport error, bad JSON is a response error, basic authentication and the sprint field are sent.

Unit-test highlights per crate:
- `vdp-core`: layering, partial overrides, typos are errors, aliases and canonical sections merge (`config.rs`); services created
  once, failures cached, cycles reported (`context.rs`); batch outcomes; table and panel rendering.
- `vdp-dbt`: guard rules and the bypasses of the old wrapper (`guard.rs`); scan rules (`argv/scan.rs`); `--vars` and `--args` reading and
  repair (`argv/yaml_arg.rs`, `argv/mod.rs`); engine exit codes and that captured runs add environment variables to the child only
  (`engine.rs`); grammar parsing from fish completions (`flags/generate.rs`); engine lookup skipping the wrapper (`locate.rs`);
  profile rendering (`profile.rs`); dbt1048 parsing (`missing.rs`); `VDP_ENV` is the canonical name (`wrapper.rs`).
- `vdp-secrets`: first provider wins, `child_env` has every key once; Vault login, token revoke once, external token never revoked,
  revoke timeout.
- `vdp-release`: policy matrix and EDROP rule (`policy.rs`), branch rules and the guard (`rules.rs`), the scan (`scan/mod.rs`: revert
  depth, pseudo-tickets, dev violations, back-merges, compare state), changelog round trip.
- `vdp-sources`: YAML tree, editor insertion, generated files, database prefix mapping.
- `vdp-cli` unit: argument parsing, help text rules (`args.rs`), the secret chain with a fake Vault (`bootstrap/secrets.rs`),
  hand-off table (`external.rs`), exit code mapping (`runtime.rs`), command helpers with fakes.

## How fakes are built

No mocking crate. Fakes are hand-written structs next to the test: `FakeVault` and `FakeJira` (a `tiny_http` server on an ephemeral
port), `FakeCatalog`/`FakeCatalogSession`, `FakeSqlSession`, `FakeDbtEngine`, `FakeTracker`. A port trait (`SqlSession`, `Vcs`,
`IssueTracker`, `Catalog`, `DbtEngine`, `SecretProvider`) is small on purpose so a fake takes a few lines; keep it that way when you
add a method (give it a default body when a sensible one exists, as `SqlSession::execute_counted` does). Git behaviour is tested
against real repositories in `tempfile` directories with the user's git configuration kept out (`GIT_CONFIG_NOSYSTEM`, explicit
identity). Process tests remove `VDP_CONFIG`, `VDP_PROJECT_DIR`, `DBT_DEPLOY_TARGET`, `VDP_LOG` from the child environment and pass
`--no-vault`.

## Not covered in this tree

The cleaned copy dropped these suites; their names remain in the README's differences table, marked with a dagger. Do not assume
the behaviour they pinned is still guarded.
- Wrapper process tests (`wrapper_process.rs`, a shell-script engine), the differential suite against the real dbt binary
  (`dbt_differential.rs`: dbt's own `eval_args` for generated invocations through the wrapper and bare) and the grammar drift test
  (`grammar_drift.rs`: the embedded grammar and `HIDDEN_ALIASES` against the engine and the dbt sources). After a change in `argv/`,
  `flags/`, `guard.rs` or `wrapper.rs` there is no automated check against a real dbt: run `vdp env doctor`, `dbt --version`, and a
  few invocations by hand (`dbt build -s x`, `dbt --select a b build`, `dbt run-operation m --args '{a:1}'`).
- Golden parity fixtures against the Python implementation (wrapper, sources YAML, referenced sources, QA) and their tests.
- Release scenario tests (a bare origin with `dev`, `main` and work clones: the `dev` invariant, re-enable, recreate losses, scope,
  promotion detection, conflict pause, changelog) and the real-repository release tests. `vdp-release` has only the unit tests above.
- Live suites (`#[ignore]`, opt-in through environment variables): the undefined-source scenario on a copy of a dbt project over a
  running SingleStore, and the `SqlSession` contract against the real ADBC driver (parameters of every kind, typed results, scripts,
  error codes, routine bodies).
- Never exercised here: Vault login against a real server, Jira, GitLab, pushes to a real remote, a database connection.

## Adding tests

- Put unit tests in the module they cover; put a process test under `crates/vdp-cli/tests/` when exit codes or output order matter.
- Test the behaviour of a rule, name the test as a sentence (`a_revert_on_dev_is_a_violation_and_cancels_the_merge`).
- A new command needs its help text to pass the tests in `args.rs` (about, long_about, `Examples:`, help per argument, width).
- A bug fix gets a test that fails without it. When you remove a behaviour of the old Python tools on purpose, add a row to the README's
  differences table and name the test that pins it.
- Scratch copies: tests work on `tempfile` directories and clean up after themselves; never point a test at a real project, a real
  remote or the shared SingleStore.
