# vdp-rust-utils

The VDP toolkit for **dbt v2** (the Rust dbt, built from `/opt/dbt` = github jimvekemans/dbt with the native
SingleStore adapter) on SingleStore. It replaces `vdp-python-utils`, which drove dbt-core v1 from Python.

One binary, `vdp`. Installed as (or linked to) `dbt`, it *is* the dbt wrapper: every argument goes to the dbt v2
engine, with the organisation's environment around it.

```
dbt build -s my_model                    # the wrapper (a `dbt` link to vdp), or: vdp dbt build -s my_model
vdp sources add edw_inc.new_table        # add a source table, with its columns, from the database
vdp sources add --missing                # add every source dbt reports as undefined (dbt1048)
vdp sources rewrite                      # regenerate models/sources/sources_<db>.yml, keep referenced tables
vdp release create --name release/2026_DROP05 --fixversion 2026_DROP05 [--dry-run] [--force [--accept-losses]]
vdp release analyse --fixversion 2026_DROP05 [--patch release/2026_DROP04] [--json]
vdp release patch release/2026_DROP05 --include INTEL-1,INTEL-2 [--dry-run]
vdp release promote release/2026_DROP05 [--kind auto|new|patch] [--dry-run] [--accept-rollback] [--accept-losses]
vdp release branches
vdp release-ui release serve [--repo-dir /opt/project] [--port 8765] [--ui-dir build]   # runs the separate `vdp-release-ui` binary (release-ui workspace)
vdp jira advance --tickets INTEL-1 INTEL-2
vdp sql run query.sql [--csv-dir out]    # vdp sql export edw_inc.t1
vdp qa orphan-tables                     # vdp qa view-candidates
vdp --env LDV vaultspeed plan            # runs the separate `vdp-vaultspeed` binary (vaultspeed-framework workspace)
vdp env show | vdp env doctor
```

Global options: `--env LDV|ABT|CBT|PRD` (aliases `dev`, `val`, `prd` are LDV, ABT, PRD; default `$DBT_DEPLOY_TARGET`, else
`[core].default_environment`, LDV unless configured), `--target`/`--profiles-dir`
(the dbt target and profiles the toolkit itself uses), `--project-dir`, `--config`, `--no-vault`. The `dbt` link
has no options of its own; it reads `$DBT_DEPLOY_TARGET`, `$VDP_CONFIG`, `$VDP_PROJECT_DIR`, `$VDP_NO_VAULT`.

## Modules

Every capability is a crate behind a trait and a cargo feature of `vdp-cli`, so a deployment can pick:

| Feature | Crates | Commands |
|---|---|---|
| always | `vdp-core` (environments, layered config, project, lazy service context), `vdp-secrets`, `vdp-db`, `vdp-dbt` | `dbt`, `vdp dbt`, `env`, `dev` |
| `sources` (default) | `vdp-sources` | `sources add`, `sources rewrite` |
| `release` (default) | `vdp-git`, `vdp-jira`, `vdp-release` | `release create\|analyse\|patch\|promote\|branches`, `jira advance` |
| `sql` (default) | `vdp-sql` | `sql run`, `sql export` |
| `qa` (default) | `vdp-qa` | `qa orphan-tables`, `qa view-candidates` |
| external | none (the `vaultspeed-framework` and `release-ui` workspaces) | `vaultspeed ...` and `release-ui ...`: `vdp` hands the command line to the `vdp-vaultspeed` and `vdp-release-ui` binaries (see below) |

`cargo build -p vdp-cli --no-default-features --features sources` builds a wrapper with only source maintenance.

Modules never name each other's concrete types. The composition root (`crates/vdp-cli/src/bootstrap`) registers
factories in the `vdp_core::Context`, and a command asks for what it needs (`Arc<dyn SecretProvider>`,
`Arc<dyn SqlSession>`, `Arc<dyn DbtEngine>`, `Arc<dyn Vcs>`, `Arc<dyn IssueTracker>`). Services are created on
first use, once. A command that only needs git never logs in to Vault. Every module crate is also a plain
library.

**The VaultSpeed Automation Framework** is not in this workspace. Its infrastructure for secrets, database and SQL
scripts lives in `vdp-secrets`, `vdp-db` and `vdp-sql-script`; the framework itself, the `vdp-vaultspeed*` crates and
`vdp-ssh` (remote access over OpenSSH), is the sibling workspace `vaultspeed-framework` (see its README). It imports
this workspace's crates by path and builds the `vdp-vaultspeed` binary: the whole `vdp` command line plus
`vaultspeed ...`, and `release serve` with the VaultSpeed release extension.

**The release UI's backend** is not in this workspace either. The release hub (`vdp-release-hub`: the HTTP API over one
clone of the dbt repository, its analyses and release jobs, sign-in and extensions) and the announcements it sends
(`vdp-notify`) are the sibling workspace `release-ui` (see its README). It builds the `vdp-release-ui` binary: the whole
`vdp` command line plus `release serve`. The release flows themselves (`vdp-release`) stay here, shared by
`vdp release ...` and the hub. `vaultspeed-framework` depends on `release-ui` (its hub extension and `release serve`),
and `release-ui` on this workspace, never the other way round.

`vdp-cli` is a library for these binaries: `args::GlobalArgs` and `args::Command` to build another command line on,
`bootstrap::context` (the services), `bootstrap::adbc_driver`, `bootstrap::resolve_secrets_from` with `SecretSources`
and `ResolvedSecrets`, `commands::run_command` for every command the other binary does not handle, and
`commands::run_dbt_validation` (the dbt validation of releases, which the hub reuses for `--validate`).

`vdp vaultspeed ...` and `vdp release-ui ...` (also with global options first: `vdp --env LDV vaultspeed plan`) replace
the `vdp` process with `vdp-vaultspeed` and `vdp-release-ui`, found next to the `vdp` executable, else on `PATH`. The
table is `external::HANDOFFS`. The original arguments are passed unchanged and in order; the one change is that
`release-ui` itself is dropped, because the binary is the release UI: `vdp release-ui release serve --port 8765` runs
`vdp-release-ui release serve --port 8765`, whereas `vdp-vaultspeed` has a `vaultspeed` command of its own and gets it.
The exit code is that of the other binary. Without it, `vdp` exits with code 2 and says so. Any other unknown subcommand
is an error as before. The same `vdp.toml` serves all binaries, so `vdp` does not warn about the `[vaultspeed]`,
`[vaultspeed_release]`, `[release_hub]` and `[notify]` sections.

## Configuration

`config/vdp.toml` documents every key with its default; a test keeps it equal to the code. Files are layered,
later ones winning key by key:

```
built-in defaults < /etc/vdp/vdp.toml < <dbt project>/vdp.toml < --config / $VDP_CONFIG
```

**Environments.** The environment is `--env`, else `$DBT_DEPLOY_TARGET` (`[core].environment_var`), else
`[core].default_environment`. The four built-in environments (`LDV`, `ABT`, `CBT`, `PRD`) are always defined. A project
with other names defines them as sections, and those names are then accepted (case-insensitively).

**Aliases.** `dev`, `val` and `prd` are aliases of the built-in `LDV`, `ABT` and `PRD` (`CBT` has none), wherever an
environment is named: `--env dev`, `$DBT_DEPLOY_TARGET=dev`, `default_environment = "dev"` and `[environments.dev]`
all mean `LDV`, so `vdp env show` prints `LDV`, and `VDP_ENV` for dbt is `LDV`. `$DBT_DEPLOY_TARGET` itself is never
rewritten, so a project's `env_var('DBT_DEPLOY_TARGET')` still sees `dev`. An alias section merges key by key into
the canonical one (`[environments.dev]` over the defaults of `LDV`); when both spellings set the same key, the alias
section wins, as `prd` always won over `PRD`.

```toml
[core]
default_environment = "dev"      # LDV

[environments.dev]               # overrides LDV's defaults key by key
vault_url = ""
ca_certificate = ""              # empty: no CA (no TLS, no Vault); `vdp env doctor` does not look for one

[environments.qa-1]              # a custom environment starts from the non-production defaults
vault_url = ""
```

**Placeholders.** `{env}` in configuration (Vault paths in `[secrets].precedence`, `[qa].run_results_table`) is the
lower-case canonical name (`ldv`, `abt`, `cbt`, `prd`; the VDAB convention), `{env_short}` the short name (`dev`,
`val`, `cbt`, `prd`; a custom environment's lower-case name for both). A project whose databases are prefixed `dev_`,
`val_`, `prd_` uses `{env_short}`: `{env_short}_vdp_mgmt.dbt_run_results`.

An unknown name stops every command with exit code 2, listing the configured ones.

**Profiles.** vdp reads the dbt profile the way dbt does. It renders only the selected target, so
`{{ var('SCHEMA', '') }}` or a missing variable in another target does not matter. `env_var()` reads the
resolved secrets. `var('X', default)` gives its default: dbt fills `var()` in a profile only from `--vars` on its
own command line, so a `var()` without a default in the selected target is an error.

**Database.** Every connection to SingleStore goes through its ADBC driver, the one dbt v2 uses
(`libadbc_driver_singlestore.so`, loaded at runtime). vdp finds it like dbt does, in `lib/dbt/` above the dbt
executable or above vdp itself (the image's `/usr/local/lib/dbt/`), unless `$VDP_ADBC_DRIVER` or `[core].adbc_driver`
names it; else the library search path. Connection settings come from the dbt profile (or, for `vdp-vaultspeed`, the
secrets) and map to the driver as in dbt v2 (`ssl_mode`, `ssl_ca`, client certificates); the password is passed as
a driver option, never in the URI.

**Secrets.** They are resolved in the precedence the Python utilities ended up with (Vault `vdp/airflow` >
`.devcontainer/.env` > Vault `vdp/<env>/dbt` > process environment):
- The Vault TLS certificate is verified with the organisation's CA (Python used `verify=False`).
- Values are never written to the process environment. Only child processes (dbt) get them.
- Modules may use their own precedence: `vdp-vaultspeed` deployments read `[vaultspeed].secrets_precedence` (its own
  Vault path, the framework's dotenv file and CA folder); Vault itself (URL, AppRole, CA name) is shared.
- Without AppRole credentials, Vault is skipped quietly. A failing Vault login warns, unless
  `[secrets].vault_required` is set.

## The dbt wrapper

**Argument fidelity is the first rule: every argv that dbt v2 accepts reaches dbt with the same meaning.**

- **Byte-for-byte forwarding.** Tokens go through unchanged as `OsString`s, including non-UTF-8 ones. The only
  values ever changed are `--args`/`--vars` values dbt would misread or reject:
  - `--vars` is read by dbt with `dbt-yaml` (the same crate, version `=0.9.6` and `yaml_11` feature as
    `/opt/dbt`). dbt already recovers `{a:1}` itself, so `--vars` is only touched when dbt would *reject* it
    and the Python wrapper's repair (a space after `:`, now only outside quotes) makes it valid: `{a:{b: 1}}`.
  - `run-operation --args` gets dbt's own `--vars` recovery, because dbt silently reads `{days:7}` as
    `{"days:7": null}` there.
  - A changed value is re-emitted as JSON whose round trip through dbt's reader is checked first.
- **The guard sees the command as dbt sees it.** The argv is scanned with dbt's own grammar: every flag of every
  (sub)command, whether it takes one value or several, and hidden aliases. That grammar is derived from the
  installed engine (`dbt completions fish` plus `<command> --help`) and cached per binary. A copy is embedded
  (`crates/vdp-dbt/grammar/dbt.json`, refreshed with `vdp dev grammar`). `dbt --debug build` and
  `dbt --target x build` are therefore guarded, `--select a b build` makes `build` a selector value (as in clap),
  and `--selector` counts as a selection.
- **The guard rule.** `build`/`run` without `-s/--select/--selector` is refused (exit 1), unless the SingleStore
  user is a service user (`*_dbt`, not `pu*`). `--help` and `--version` are never refused.
- **Transparent process.** dbt shares the terminal. The wrapper survives Ctrl-C and forwards SIGTERM, SIGHUP and
  SIGQUIT. The exit code passes through, and a death by signal maps to 128+n.
- **Environment for dbt.** dbt also gets `DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true` (needed for SingleStore in
  dbt v2), `VDP_ENV` and `VDP_SSL_CA` (the environment's CA, for `ssl_ca:` in profiles.yml).
- **Hint for undefined sources.** After a failed run with undefined sources (dbt1048), it prints
  `hint: vdp sources add <source>.<table> ...`.

## Sources

`vdp sources add --missing` runs `dbt parse --log-format json` and collects the dbt1048
`Source '<source>.<table>' not found` events. Each table is then looked up in `information_schema`, with bound
parameters. Where the table goes depends on how its source is declared:
- **Hand-written YAML file** (with anchors, comments, any indentation): the table is inserted as text after the
  source's last table, in the file's own indentation. The edited file is parsed again and must equal the old one
  plus exactly the new table, or nothing is written.
- **Not declared yet:** it goes to `models/sources/sources_<db>.yml`, written in the same layout as the Python
  `DbtSourceDumper`. These generated files keep the descriptions, tests and meta people added; Python dropped them.
- **Table not in the database:** reported with close matches, nothing written.

Afterwards it runs `dbt parse` again and confirms nothing is undefined any more.

**Databases named per environment** (`dev_landing`, `val_landing`, `prd_landing`): `[sources.database_prefix]` maps
the databases vdp reads to what the source files say. `add` and `rewrite` both use it.

```toml
[sources.database_prefix]
physical = "dev_"                                       # the databases vdp connects to
jinja = "{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_"    # what the source files say instead
```

- `vdp sources add dev_landing.t` writes the source `landing` to `models/sources/sources_landing.yml`, with
  database and schema `{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_landing`.
- `vdp sources rewrite` writes the same file for `dev_landing` in `[sources].databases`.
- A hand-written source whose database is that expression (whitespace and quote style do not matter) is read from
  `dev_landing`, so `--missing` can add its tables. Without the option, such a source is left to the developer, as
  before.

## Differences from vdp-python-utils

Each difference is intentional. The test that pins it is named in the last column.

| Python | Now | Test |
|---|---|---|
| Guard read only `argv[0]` (`dbt --debug build` ran everything); `--selector` did not count | dbt's grammar; `--selector` counts | `guard::tests`, `golden_wrapper.rs`, `dbt_differential.rs` |
| `--vars`/`--args` were pulled out, re-dumped and moved to the end. The colon repair applied only when *every* key was broken, and never inside nested maps | Tokens stay in place and byte-identical; dbt's reader decides; only values dbt misreads/rejects are fixed | `yaml_arg::tests`, `golden_wrapper.rs` |
| Invalid `--vars` → Python exited 1 itself | dbt reports it (exit 2) | `golden_wrapper.rs` |
| `log(level="ERROR")` raised, so cleanup and exit paths never ran | typed errors, exit 0/1/2 | throughout |
| Vault `verify=False`; secrets exported to `os.environ` | CA-verified; child processes only | `wrapper_process.rs` |
| `dbt` ran from `models/` crashed (`Repo(cwd)`) | project found by walking up | `project::tests` |
| Source files lost descriptions/tests/meta on rewrite; `databases=None` crashed | metadata kept; databases from config | `generated::tests` |
| Hand-written source files could not be extended | span-preserving insertion | `editor::tests`, `live_sources.rs` |
| SQL built by string formatting (`download_from_singlestore`, introspection) | bound parameters, quoted identifiers | `vdp-sql`, `catalog.rs` |
| SQLAlchemy/mysqlclient (Python), the `mysql` crate (the VaultSpeed framework) | every connection goes through SingleStore's ADBC driver, the one dbt v2 uses (`libadbc_driver_singlestore.so`, found like dbt finds it); profile TLS settings map as in dbt v2; a script returns its last result set | `vdp-db` (`tests/live_adbc.rs`) |
| Release: EDROP fix versions sorted after DROPs and compared with the target | any EDROP fix version keeps the ticket, in releases and patches (EDROP numbers run on their own) | `policy::tests` |
| Release UI patch: cherry-picked only the feature tip, so multi-commit features lost commits | the ticket's mainline merges with `-m 1` (the whole feature); a ticket reverted on the branch is re-enabled by reverting the revert | `scenario.rs` |
| Revert conflicts (a file changed on one side, deleted on the other) stopped the release; cherry-picks deleted the file | the CLI resolves leftover conflicts for the change being applied (`-X theirs` semantics) and names the files; the release UI pauses the job and the release manager decides per file (or a suggested selection change); `strict` surfaces every hunk | `real_repo.rs`, `scenario.rs`, `api.rs` |
| Release UI: promotion pushed main with `--force`; release branches sorted as strings | `--force-with-lease` bound to the head the preview saw (a push in between is refused, never overwritten); sorted by fix version (numerically) | `scenario.rs` (lease) |
| Nothing stopped a release branch named `dev`, a patch of `dev`, or a revert with HEAD on `dev` | `dev` is never written: branch names are validated (`<branch_prefix>*` only), and `GitCli` refuses writes and pushes outside `release/*` and `main`, and on a detached HEAD; the hub's clone keeps no local `dev`. Reverts found on `dev` (GitLab revert MRs, back-merges of `release/*`/`main`) are reported, never repeated | `nothing_ever_writes_dev`, `real_repo.rs` |
| Any message containing "revert" was a revert; a re-enable (`Revert "Revert …"`) counted as a revert, so the next patch removed the feature again | a revert graph: git's `This reverts commit` trailer, else the `Revert "`/`Reapply "` depth; per ticket the net state on the compare branch | `scan::tests`, `a_re_enabled_ticket_stays_enabled` |
| A GitLab "Revert" MR merge counted as a second merge of the ticket; merges without a ticket (`Merge branch 'fusion'`) were invisible and always shipped | revert MRs and back-merges are classified by what they bring; ticketless merges are `branch:<name>` pseudo-tickets (kept by default, revertable) | `scan::tests` |
| A revert on main of a ticket without a fix version was not carried over | carried over while the ticket is still out by policy (`keep_empty` decides) | `a_revert_of_a_ticket_without_fix_version_is_carried_over` |
| Promotion kind from main's last commit message; promoting an older release rolled main back silently; recreate and new promotion dropped commits silently | detection by ancestry (nothing to promote, patch, new); a rollback needs `--accept-rollback`; dropped commits are listed and need `--accept-losses` | `promotion_is_detected_by_ancestry_and_guarded`, `recreating_a_release_shows_what_it_drops_and_binds_to_the_preview` |
| Any release branch could be changed | only the newest two (`--allow-old-release` in the CLI); a promoted branch is patched, not recreated | `only_the_newest_two_release_branches_can_change` |
| No record on the branch of what a release contains | `changelog/<DROP>.md` (accepted, added, excluded, patches; JSON front matter) is the branch's last commit; a patch rewrites it at the tip; releases carry the others forward from main | `the_changelog_is_the_last_commit_and_a_patch_rewrites_it` |
| Push failures were raw git text; git could hang forever | `PushRejection` (non-fast-forward, stale lease, protected, hook, auth, network), verified by a fetch; every git call has a timeout | `real_repo.rs` |
| Release UI: Jira and SMTP credentials partly sent by the browser | only on the server (secret chain); the API reports readiness | `vdp-release-hub` `api.rs` (release-ui workspace) |
| Release UI: a commit inside a merged feature naming another ticket became a ticket of its own (its commit reverted separately) | a mainline merge belongs to the first ticket in its message (the branch's); dbt-vdp's INTEL-27178 (inside the INTEL-27128 merge) is the one case in the current history | parity run |
| Release: merges scanned newest-first, so a revert was seen before its merge and the reverted ticket stayed in scope; feature commits were counted besides merges; ticket ids cut to 11 characters | mainline only, oldest first; whole ids | `scan::tests`, `scenario.rs` |
| Fix versions compared as strings (`DROP5` > `DROP10`); "Nog in te plannen" never matched, so `--keep-empty-fixversions` kept it | numeric drop number; exclusions apply | `fix_version`, `policy::tests` |
| A Jira lookup failure counted as "no fix version" → silently reverted | `release create` stops and names the tickets; an analysis (and the UI board) marks them per ticket | `scenario.rs` |
| Release branch pushed right after creation, before `dbt compile` | pushed only after validation; dry run changes nothing local | `scenario.rs` |
| Work tree auto-stashed silently | refused when dirty | `flow.rs` |
| Jira transition by list index; credentials prompted with `input()` | by name (default: the first, as before); secrets only | `jira advance` |
| QA orphans counted tests as readers | tests are not readers | `golden_qa.rs` |

**Migration notes** (dbt v2 itself, not this toolkit):
- dbt v2 reads `--vars '{a: yes, c: 007}'` as strings `"yes"`/`"007"`; dbt-core v1 read `true`/`7`.
- profiles.yml needs `type: singlestore` with the v2 fields (`ssl_mode`, `ssl_ca: "{{ env_var('VDP_SSL_CA') }}"`).
- The test project still uses the model config `chunk_by`, which the backfill branch renamed to
  `chunk_output_by`.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets` (pedantic, clean),
  `cargo test --workspace --locked`, `RUSTDOCFLAGS=-Dwarnings cargo doc`, plus each feature set.
- Golden parity with the Python code: `tools/golden_master.py` runs the real Python functions and writes
  `fixtures/golden/*.json` (wrapper, sources YAML, referenced sources, QA). The golden tests list each difference.
- Against the real dbt v2 binary (opt-in, `VDP_DBT_BIN=/opt/dbt/target/release/dbt`):
  - **`dbt_differential.rs`:** every catalogue case plus 400 generated invocations run through the wrapper and
    bare. dbt's own `eval_args` (command, select, selector, vars) must be identical, and exit codes and
    `--args` (echo macro) must match.
  - **`grammar_drift.rs`:** the embedded grammar equals the engine's, and the hidden aliases equal the dbt
    sources (`VDP_DBT_SRC=/opt/dbt`).
- Against a dbt project and SingleStore (`VDP_LIVE_PROJECT=/home/jim/Documents/dbt-singlestore-testing`):
  `live_sources.rs` runs the whole undefined-source scenario on a copy of the project, over the ADBC driver.
- Against SingleStore through the real ADBC driver (opt-in, `VDP_ADBC_DRIVER`, `VDP_DB_PASSWORD`):
  `vdp-db/tests/live_adbc.rs` (the session contract: bound parameters of every kind, typed results, scripts and
  their last result set, MySQL error codes, routine bodies).
  The VaultSpeed framework's live tests are in its own workspace.
- External subcommands (`crates/vdp-cli/tests/external.rs`): `vdp vaultspeed ...` and `vdp release-ui ...` against
  stand-ins for `vdp-vaultspeed` and `vdp-release-ui` on `PATH`: the original arguments (global options first included;
  without `release-ui` itself for that command) and the exit code pass through; a missing binary is exit code 2 with
  an explanation; another unknown subcommand is clap's error.
- The VaultSpeed framework (deployer, planner, object history, VaultSpeed releases and their hub extension) is tested in
  the `vaultspeed-framework` workspace.

- The release hub (API over HTTP on real repositories, clone lifecycle, extensions) and the announcements (HTML and text
  byte-identical to the Python `ReleaseEmailBuilder`; SMTP against a local stub server) are tested in the `release-ui`
  workspace, with the `release serve` process test.
- Release flows on real data: a local bare copy of dbt-vdp (12 release branches, 74 tickets since the merge-base)
  driven through `vdp-release-ui release serve` and the Svelte UI with a stub Jira and a stub GitLab: release `2026_DROP25`
  with two conflicts decided in the UI, repair after a restart, recreate with a lease, a new promotion listing the
  fix only main had, a patch re-enabling a ticket with the changelog rewritten, the patch promotion. `dev`
  unchanged throughout.

Not exercised against live services here: Vault login, Jira, SMTP, the Airflow host, the GitLab API (rights,
protected branches) and pushes to git.example.com.
Those adapters are tested through fakes, real temporary git repositories, and stand-in servers.
