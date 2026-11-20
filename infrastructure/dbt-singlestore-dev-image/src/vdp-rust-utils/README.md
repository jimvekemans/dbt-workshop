# vdp-rust-utils

The Cargo workspace of **vdp**, a toolkit for **dbt v2** (the Rust dbt, built from the fork in the base image's submodule
`infrastructure/dbt-singlestore-base-image/src/dbt`, with the native SingleStore adapter) on SingleStore. It replaces
`vdp-python-utils`, which drove dbt-core v1 from Python.

One binary, `vdp`. Installed as (or linked to) `dbt`, it *is* the dbt wrapper: every argument goes to the dbt v2 engine, with the
environment, the secrets and a selection guard around it. The same binary maintains dbt sources, runs the DROP release flows
(with Jira), runs ad-hoc SQL and checks the dbt manifest.

```
dbt build -s my_model                    # the wrapper (a `dbt` link to vdp), or: vdp dbt build -s my_model
vdp sources add landing.new_table        # add a source table, with its columns, from the database
vdp sources add --missing                # add every source dbt reports as undefined (dbt1048)
vdp sources rewrite                      # regenerate models/sources/sources_<db>.yml, keep referenced tables
vdp release create --name release/2026_DROP05 --fixversion 2026_DROP05 [--dry-run] [--force [--accept-losses]]
vdp release analyse --fixversion 2026_DROP05 [--patch release/2026_DROP04] [--json]
vdp release patch release/2026_DROP05 --include INTEL-1,INTEL-2 [--dry-run]
vdp release promote release/2026_DROP05 [--kind auto|new|patch] [--dry-run] [--accept-rollback] [--accept-losses]
vdp release branches
vdp jira advance --tickets INTEL-1 INTEL-2
vdp sql run query.sql [--csv-dir out]    # vdp sql export landing.orders
vdp qa orphan-tables                     # vdp qa view-candidates
vdp env show | vdp env doctor
```

## 1. Where it sits

Three Cargo workspaces live next to each other under `infrastructure/dbt-singlestore-dev-image/src/`. The later ones depend on the
earlier ones **by path** (`../vdp-rust-utils/crates/<name>`), never the other way round:

```
vdp-rust-utils  <--  release-ui  <--  vaultspeed-framework
binary `vdp`         binary           binary `vdp-vaultspeed`
(this workspace)     `vdp-release-ui` (VaultSpeed deployments)
                     (release hub)
```

- Only `vdp` is built into an image: the dev image `dbt-singlestore-dev:1.11.11` (`vdp-build` stage of its `Dockerfile`), where `dbt` on
  the `PATH` is a link to `vdp` and the real engine sits in `/usr/local/libexec/dbt/dbt`. The devcontainer of the workshop project uses
  that image.
- `release-ui` and `vaultspeed-framework` are built by hand (they are in no image). `vdp vaultspeed ...` and `vdp release-ui ...` replace
  the `vdp` process with `vdp-vaultspeed` / `vdp-release-ui` when those binaries are next to `vdp` or on `PATH`; otherwise `vdp` says so
  and exits with code 2, which is what happens in the dev image.
- A public API change here can break the two siblings without the image build noticing: build both after such a change (section 8).

**This tree is a cleaned copy.** Left out: documentation other than this README, test fixtures, the sample configuration, the golden,
differential and live test suites, and the tooling that produced them. E-mail addresses and host names of the organisation the toolkit
was written for are replaced by placeholders (`person1@example.com`, `git.example.com`), and `Cargo.toml` still names the original
internal licence (`LicenseRef-VDAB-internal`). The untouched originals are kept locally in `infrastructure/dbt-singlestore-dev-image/local/`
(gitignored); they describe a larger code base. Sections 8 and 9 say what that means for testing and which gaps remain.

## 2. Build, install, run

**On a host** with `rustup` (the pinned toolchain `1.96` with `rustfmt` and `clippy` installs itself through `rust-toolchain.toml`):

```bash
cargo build --release --locked -p vdp-cli        # target/release/vdp
cargo build -p vdp-cli --no-default-features --features sources   # only source maintenance (plus env, dev, dbt)
```

At run time `vdp` needs a dbt v2 engine (found through `[dbt].binary`, `$VDP_DBT_BIN`, or a `dbt` on `PATH` that is not `vdp` itself) and, for
anything that talks to the database, SingleStore's ADBC driver `libadbc_driver_singlestore.so` (loaded at run time; found as dbt finds it:
`lib/dbt/` in a directory above the engine or above `vdp`, unless `$VDP_ADBC_DRIVER` or `[core].adbc_driver` names it). Both come from the
base image, which builds them from its submodules.

**Install as `dbt`.** `vdp` is multicall: when its file name is `dbt` (or `dbt.exe`) it is the wrapper and every argument belongs to dbt.

```bash
install -m 755 vdp /usr/local/bin/vdp
mv /usr/local/bin/dbt /usr/local/libexec/dbt/dbt        # the real engine (create the directory first)
ln -s vdp /usr/local/bin/dbt
export VDP_DBT_BIN=/usr/local/libexec/dbt/dbt           # so the wrapper never finds itself
```

The `dbt` link has no options of its own; it reads `$DBT_DEPLOY_TARGET`, `$VDP_CONFIG`, `$VDP_PROJECT_DIR` and `$VDP_NO_VAULT` (read as `vdp` reads it).

**With Docker.** The dev image builds vdp from this folder:

```bash
docker build -t dbt-singlestore-dev:1.11.11 infrastructure/dbt-singlestore-dev-image      # or `make images` at the repository root
docker build --build-arg CARGO_BUILD_JOBS=2 -t dbt-singlestore-dev:1.11.11 infrastructure/dbt-singlestore-dev-image
```

- A prebuilt release binary `vdp` in the image folder (gitignored) is copied in as is and wins over the source build. Make it with
  `cargo build --release --locked -p vdp-cli` in this folder and `cp target/release/vdp ../../vdp`; it must run on the image's glibc (the
  build stage uses Debian trixie for that reason), so a host build on a newer system may not run. A stale prebuilt wins silently: source
  edits reach the image only after the file is rebuilt or deleted.
- Without it the stage runs `cargo build --release --locked -p vdp-cli` in a `rust:1.96-trixie` image (`--build-arg RUST_VERSION=...`
  changes the Rust image tag, `--build-arg BASE_IMAGE=...` the base image, `CARGO_BUILD_JOBS` limits parallelism on a busy host).
- With neither, the image is built without vdp and `dbt` stays the plain engine.
- Only the default features are built (`sources`, `release`, `sql`, `qa`).
- A running devcontainer keeps the image it was created from: rebuild or recreate it (VS Code: Dev Containers: Rebuild Container) to
  pick up a new image.

**The siblings** are built by hand, in their own folders, and their binaries go next to `vdp` or on `PATH`:

```bash
(cd ../release-ui && cargo build --release --locked -p vdp-release-ui-cli)               # vdp-release-ui
(cd ../vaultspeed-framework && cargo build --release --locked -p vdp-vaultspeed-cli)     # vdp-vaultspeed
```

`vdp vaultspeed plan` (also with global options first: `vdp --env LDV vaultspeed plan`) and `vdp release-ui release serve --port 8765`
pass the original arguments on, in order. The one change: `release-ui` itself is dropped, because the binary *is* the release UI
(`vdp-release-ui release serve --port 8765`), whereas `vdp-vaultspeed` has a `vaultspeed` command of its own and gets it. The exit code
is that of the other binary. The table is `external::HANDOFFS`; the same `vdp.toml` serves all binaries, so `vdp` does not warn about the
`[vaultspeed]`, `[vaultspeed_release]`, `[release_hub]` and `[notify]` sections.

## 3. Command reference

`vdp -h` is a summary, `vdp <command> --help` has the details and examples. Global options come before or after the command.

### Global options

| Option | Meaning |
|---|---|
| `--env <ENV>` | `LDV`, `ABT`, `CBT`, `PRD`, or a name configured as `[environments.<name>]`; `dev`, `val`, `prd` are aliases of `LDV`, `ABT`, `PRD` (`CBT` has none). Default: `$DBT_DEPLOY_TARGET`, else `[core].default_environment`. An unknown name is exit 2. |
| `--target <TARGET>` | The dbt target the toolkit itself uses for its connections and its own dbt runs (`dbt parse`, the release validation). Default: the profile's `target`. It does not change what `vdp dbt ...` passes to dbt. |
| `--profiles-dir <DIR>` | Directory with `profiles.yml`. Default: the first of `$DBT_PROFILES_DIR`, the dbt project directory and `~/.dbt` that has one. |
| `--project-dir <DIR>` (`$VDP_PROJECT_DIR`) | The dbt project directory (the one with `dbt_project.yml`). Default: the nearest parent of the current directory that has one. Its `vdp.toml` and dotenv file are read. |
| `--config <FILE>` (`$VDP_CONFIG`) | An extra TOML file, layered last. Unlike the other files it must exist. |
| `--no-vault` (`$VDP_NO_VAULT`) | Do not log in to Vault: secrets come from the dotenv file and the process environment. As a variable it takes `1`, `true`, `yes`, `on` (Vault off) or `0`, `false`, `no`, `off` (Vault on), whatever the case; an empty value counts as unset and anything else is an error (exit 2). |
| `-v`, `--verbose` | Debug logging on stderr (and in the `--log-json` file). `$VDP_LOG`, when set, overrides it. |
| `-q`, `--quiet` | Warnings and errors only on stderr. The `--log-json` file still gets info events. |
| `--log-json <FILE>` (`$VDP_LOG_JSON`) | Also write the log as NDJSON (one event per line: timestamp, level, target, message) to this file, replacing an existing one; created with mode 0600. `info` and above by default, `debug` with `-v`, whatever `-q` says; `$VDP_LOG` overrides both sinks. Not used by `vdp dbt`. |
| `-h`, `--help`, `-V`, `--version` | Help (`-h` summary, `--help` details) and version. |

### Commands

| Command | Arguments and options | What it does |
|---|---|---|
| `vdp dbt <DBT_ARGS>...` | everything after the first non-global argument goes to dbt unchanged | The wrapper (section 6). `vdp dbt --help` is dbt's own help; `vdp help dbt` shows vdp's text. Exit code: dbt's (128+n for a signal); 1 when the guard refuses; 2 when setup fails. |
| `vdp sources add [TABLE]...` | `TABLE`: `database.table`, or `source.table` for a source declared by hand (physical database name where `[sources.database_prefix]` is set); `--missing`: run `dbt parse` and add every table it reports as undefined (dbt1048), then parse again; `--dry-run`: print `would add ...` lines | Adds tables, with their columns, to source YAML files (section 6). Changes only YAML. Exit 1: a table could not be added or sources are still undefined; 2: nothing to add, or `dbt parse` fails for another reason. |
| `vdp sources rewrite` | `--databases <DATABASE>`: comma-separated physical names (default `[sources].databases`); `--no-trim`: no `dbt parse`, keep unreferenced tables; `--dry-run`: list per database what would be written | Regenerates `sources_<database>.yml` for each database, then trims to the tables the project references; a file left with none is deleted. Hand-written sources are skipped. Exit 2: `dbt parse` fails (the files stay untrimmed). |
| `vdp release create` | `--name <BRANCH>` and `--fixversion <FIXVERSION>` (required); `--from <REF>` (default `[release].from`); `--to <REF>` (default `[release].to`); `--keep-empty-fixversions`; `--interactive`; `--force`; `--accept-losses`; `--no-changelog`; `--allow-old-release`; `--dry-run` | Cuts a release branch from `--from`: reverts the tickets outside the target DROP (classified from Jira), commits `changelog/<fixversion>.md`, runs `[release].validate` with dbt, then pushes. `--force` recreates an existing branch with a lease; commits only the old branch has need `--accept-losses`. |
| `vdp release analyse` | `--fixversion <FIXVERSION>` (required unless `--patch`); `--from <REF>`; `--to <REF>`; `--patch <BRANCH>`; `--json` | Shows what a release, or with `--patch` a patch, would contain: each ticket `revert` or `keep` (patch: `include`), with the reason. `--json` prints the analysis on stdout. Changes nothing. |
| `vdp release patch <BRANCH>` | `--include <TICKET>` (comma-separated; default: emergency drops and re-enable candidates); `--fixversion <FIXVERSION>` (default: the branch suffix); `--from <REF>`; `--interactive`; `--allow-old-release`; `--no-changelog`; `--dry-run` | Adds tickets to a release branch (re-enables reverted ones, cherry-picks new ones as whole mainline merges), rewrites the changelog as the last commit and pushes without force, with a lease. |
| `vdp release promote <BRANCH>` | `--kind auto\|new\|patch` (default `auto`, by ancestry); `--accept-rollback`; `--accept-losses`; `--allow-old-release`; `--dry-run` | Brings a release branch to `main`: a new release resets `main` to the merge-base and merges `--no-ff`; a patch merges onto `main`. Pushed with a lease on `main` as it was. Reads no Jira. |
| `vdp release branches` | none | Lists the release branches on origin, newest first (by fix version, numerically): name, first 10 characters of the head, `on main` when promoted. |
| `vdp jira advance` | `--tickets <TICKET>...` (required); `--transition <NAME>` (default `[jira].advance_transition`, else the first transition Jira offers); `--dry-run` | Moves tickets to their next status; a ticket that cannot be moved is reported and does not stop the others. Exit 1: some tickets could not be advanced; 2: Jira unreachable or the credentials are refused (the batch stops; earlier tickets stay moved). |
| `vdp sql run <FILE>...` | `--csv-dir <DIR>`; `--limit <ROWS>` (default 50); `--dry-run` | Sends each file whole to the database with the rights of the profile's user (nothing is made read-only); shows the last result set as a table, and with `--csv-dir` saves it in full as `<file stem>.csv`. `--dry-run` connects to nothing. Exit 2: SQL or connection error (earlier files have run). |
| `vdp sql export <TABLE>` | `--out <FILE>` (default `data/<database>_<table>.csv`) | Saves `SELECT * FROM <database>.<table>` (name validated and quoted) as CSV with a header; prints the row count. |
| `vdp qa orphan-tables` | none | Lists table models that nothing but tests reads. Runs `dbt parse` first when there is no manifest. Exit 0 whether or not orphans exist; 2 when `dbt parse` fails. |
| `vdp qa view-candidates` | `--out <FILE>` (default `dbt_view_candidates.csv`); `--no-timings` | Ranks views by the build time of the tables downstream of them (from the latest run in `[qa].run_results_table`); prints the top ten and writes all candidates, with a dbt selector each, to a CSV. `--no-timings` makes no connection and every time 0. Runs `dbt parse` first when there is no manifest. Exit 2: `dbt parse` fails, or the run-results table cannot be read. |
| `vdp env show` | none | Prints the environment, project, configuration files read, CA certificate, Vault URL and login result, the secret sources in precedence order and every secret key with its source. Never values. |
| `vdp env doctor` | none | One `ok` or `FAIL` line per check (below); exit 1 when any fails. Does not connect to the database. |
| `vdp dev grammar` | `--dbt <PATH>` (default: the engine located as for `vdp dbt`); `--out <FILE>` (default stdout) | Derives dbt's command-line grammar from a dbt binary, as JSON, to refresh `crates/vdp-dbt/grammar/dbt.json`. Runs that binary. |
| `vdp vaultspeed ...`, `vdp release-ui ...` | whatever the other binary takes | Hand-offs (section 2). Exit 2 when the binary is not found. |

All `release` commands work on the git repository of the dbt project and fetch `origin` first (a failed fetch is a warning). Only release
branches are written (`main` only by `promote`), never `dev`; a command that writes refuses a work tree with uncommitted changes; unless
`--allow-old-release` is given only the newest two release branches can change. Jira credentials are the secrets named in `[jira]`; they
are never prompted for. `create`, `patch` and `promote` refusals and failures (existing branch, dirty work tree, failed validation,
rejected push, Jira error) are exit 2.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | The command ran, but some of its work failed (a batch with failures, a failed check), or `dbt` returned its own code 1. |
| 2 | Setup or usage error: bad arguments, unreadable configuration, unknown environment, a service that cannot be reached, a refused operation. Errors print as `vdp: error: <message>`. The one refusal that is exit 1: the wrapper's guard refusing `dbt build` or `dbt run` without a selector. |

`vdp dbt` and the `dbt` link return dbt's own exit code (128+n when dbt dies by signal n). Commands whose codes differ say so in their help.

### Environment variables

| Variable | Used for |
|---|---|
| `DBT_DEPLOY_TARGET` | Selects the environment (the name is `[core].environment_var`). Never rewritten: dbt sees it as given (`dev`). |
| `VDP_PROJECT_DIR`, `VDP_CONFIG`, `VDP_NO_VAULT`, `VDP_LOG_JSON` | The same as `--project-dir`, `--config`, `--no-vault`, `--log-json`. The `dbt` link reads the first three. `$VDP_NO_VAULT` is one boolean for `vdp` and the link (`args::parse_no_vault`): see `--no-vault`; a value that is not a boolean makes both exit 2. |
| `VDP_LOG` | A `tracing` filter (`debug`, `warn,vdp_dbt=debug`, ...). Overrides the level of every log sink. |
| `VDP_DBT_BIN` | The dbt engine, after `[dbt].binary` and before the `PATH` search. |
| `VDP_ADBC_DRIVER` | The SingleStore ADBC driver file; wins over `[core].adbc_driver`. |
| `DBT_PROFILES_DIR` | Where `profiles.yml` is looked for, after `--profiles-dir`. |
| `DBT_LOG_PATH` | Where the wrapper looks for `dbt.log` (after `--log-path`) to print the undefined-source hint. |
| `CI_PROJECT_DIR` | When set, `vdp` is "in CI": the assets directory defaults to `/usr/app/dbt/assets` first. |
| `XDG_CACHE_HOME`, `HOME` | The grammar cache directory; `HOME` also locates `~/.dbt`. |
| `PATH` | Finds the dbt engine (the first `dbt` that is not `vdp`) and the hand-off binaries. |
| `KEYVAULT_ROLE_ID`, `KEYVAULT_SECRET_ID`, and the `_LDV_ABT` / `_CBT_PRD` variants | The AppRole credentials, read from the dotenv file or the process environment. The names are `[environments.<name>].role_id_vars` and `secret_id_vars`. |
| `JIRA_USER`, `JIRA_TOKEN` | Jira account and token, as secrets (the names are `[jira].user_secret` and `token_secret`). |
| `SINGLESTORE_USER` | The secret the guard reads to tell a service user from a personal one (`[dbt].user_secret`). |
| `SINGLESTORE_HOST`, `_USER`, `_PASS`, `_DB`, ... | Not read by `vdp` itself: it connects from the dbt profile. A profile typically reads them through `env_var()`. The library reads synonyms (`host`/`server`/`url`, `user`/`username`, `pass`/`password`/`pwd`, `dbname`/`db`/`database`, `port`) for callers that build connections from secrets, such as the VaultSpeed sibling. |
| `HTTPS_PROXY` and the other proxy variables the HTTP client reads | Used by the Vault and Jira clients. |
| Set for dbt | `DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true` (from `[dbt].env`, unless already set), `VDP_ENV` (the canonical environment name, `LDV` for `dev`), `VDP_SSL_CA` (the environment's CA file, when it exists). |

**Grammar cache.** For each engine binary the wrapper derives dbt's argument grammar once and caches it in
`~/.cache/vdp/dbt-grammar-<hash>.json` (`$XDG_CACHE_HOME/vdp/` when set). `<hash>` is a 16-digit hexadecimal FNV-1a hash of the binary's
real path, size and modification time, so a new engine builds a new file. The file is written through a temporary file and a rename. When
the grammar cannot be derived (not dbt v2, binary cannot run) the embedded copy is used. `[dbt].grammar = "embedded"` skips the cache.

**`env doctor` checks:** `dbt engine` (the binary runs and prints a version), `argument grammar` (whether the engine's version matches the
embedded copy; informational), `singlestore driver` (the file exists), `experimental adapters` (`DBT_ALLOW_EXPERIMENTAL_ADAPTERS` is set in
the process or in `[dbt].env`), `ca certificate` (the file exists; an empty `ca_certificate` needs none), `dbt project` (a `dbt_project.yml` is
found), `vault` (the login worked, or was skipped on purpose; an attempted login that failed is a FAIL).

## 4. Configuration reference

Files are layered, later ones winning key by key:

```
built-in defaults < /etc/vdp/vdp.toml < <dbt project>/vdp.toml < --config / $VDP_CONFIG
```

Tables merge key by key; scalars and arrays are replaced whole. The three files are optional, the `--config` file must exist. The
core sections (`core`, `secrets`, `environments`) are checked on every run; a module section is checked when a command reads it, so a bad
key in `[dbt]` fails `vdp env doctor` but not `vdp env show`. Every settings struct rejects unknown keys (exit 2, naming the section). A top-level section that no module claims only warns
(`configuration sections no module reads (typo?)`); claimed are `core`, `secrets`, `environments`, `dbt`, `sources`, `git`, `jira`,
`release`, `sql`, `qa` and the sibling binaries' `vaultspeed`, `vaultspeed_release`, `release_hub`, `notify`.

**Environments.** The environment is `--env`, else `$DBT_DEPLOY_TARGET` (`[core].environment_var`), else `[core].default_environment`. The
four built-in environments (`LDV`, `ABT`, `CBT`, `PRD`) are always defined. A project with other names defines them as sections, and
those names are then accepted (case-insensitively; letters, digits, `_` and `-`). An unknown name stops every command with exit code 2,
listing the configured ones.

**Aliases.** `dev`, `val` and `prd` are aliases of the built-in `LDV`, `ABT` and `PRD` (`CBT` has none), wherever an environment is named:
`--env dev`, `$DBT_DEPLOY_TARGET=dev`, `default_environment = "dev"` and `[environments.dev]` all mean `LDV`, so `vdp env show` prints
`LDV`, and `VDP_ENV` for dbt is `LDV`. `$DBT_DEPLOY_TARGET` itself is never rewritten, so a project's `env_var('DBT_DEPLOY_TARGET')` still
sees `dev`. An alias section merges key by key into the canonical one (`[environments.dev]` over the defaults of `LDV`); when both
spellings set the same key, the alias section wins.

```toml
[core]
default_environment = "dev"      # LDV

[secrets]
precedence = ["dotenv", "env"]   # no `vault:` entry: Vault is never contacted

[environments.dev]               # overrides LDV's defaults key by key
vault_url = ""                   # unused without a `vault:` entry (with one, an empty URL is a failed login)
ca_certificate = ""              # empty: no CA file; `vdp env doctor` does not look for one

[environments.qa-1]              # a custom environment starts from the non-production defaults
vault_url = ""
```

**Placeholders.** `{env}` and `{env_short}` are replaced in the Vault paths of `[secrets].precedence` and in `[qa].run_results_table`:
`{env}` is the lower-case canonical name (`ldv`, `abt`, `cbt`, `prd`), `{env_short}` the short name (`dev`, `val`, `cbt`, `prd`; a custom
environment's lower-case name for both). A project whose databases are prefixed `dev_`, `val_`, `prd_` uses `{env_short}`:
`{env_short}_vdp_mgmt.dbt_run_results`.

**Secrets.** `[secrets].precedence` lists the sources, highest first: `vault:<path>`, `dotenv` (the file `[core].dotenv`, parsed without
touching the process environment), `env` (the process environment). The default is the precedence the Python utilities ended up with:
Vault `vdp/airflow` > dotenv > Vault `vdp/{env}/dbt` > process environment.
- Vault is used with an AppRole login whose ids come from the dotenv file or the process environment. The Vault TLS certificate is
  verified with the environment's CA (the Python code used `verify=False`). The token is revoked right after reading.
- Without AppRole credentials, Vault is skipped quietly. A failing login or read warns and the chain goes on without Vault, unless
  `[secrets].vault_required` is set. `--no-vault` skips Vault.
- Values are never written to the process environment. Only child processes (dbt) get them.
- Modules may use their own precedence: the VaultSpeed sibling reads its own section; Vault itself (URL, AppRole, CA name) is shared.

**Profiles.** `vdp` reads the dbt profile the way dbt does. It renders only the selected target, so `{{ var('SCHEMA', '') }}` or a missing
variable in another target does not matter. `env_var()` reads the resolved secrets, then the process environment. `var('X', default)` gives
its default: dbt fills `var()` in a profile only from `--vars` on its own command line, so a `var()` without a default in the selected target
is an error.

**Database.** Every connection to SingleStore goes through its ADBC driver, the one dbt v2 uses (`libadbc_driver_singlestore.so`, loaded at
run time). Connection settings come from the dbt profile (or, for the VaultSpeed sibling, the secrets) and map to the driver as in dbt v2
(`ssl_mode`, `ssl_ca`, client certificates; certificates without `ssl_mode` mean `verify_ca`); the password is passed as a driver option,
never in the URI.

The keys of every section follow. The same tables (the block between the `config-tables` comments) are in
`.agents/skills/vdp-rust-utils/references/config.md`; keep the two copies equal. `crates/vdp-cli/tests/config_reference.rs` fails when the copies
differ, or when a section, a key or a default here is not what the `*Settings` structs say.

<!-- config-tables:start -->
**`[core]`** (`CoreSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `default_environment` | environment name | `"LDV"` | Environment when neither `--env` nor the variable named by `environment_var` selects one. Must be defined (built-in, alias or `[environments.<name>]`), else exit 2. |
| `environment_var` | string | `"DBT_DEPLOY_TARGET"` | Process variable that selects the environment. |
| `dotenv` | path | `".devcontainer/.env"` | Dotenv file with secrets, relative to the dbt project root. A missing file is an empty source; a malformed line is an error. |
| `assets_dir` | path | `""` | Directory with the CA certificate files. Empty: the first existing of `/usr/app/dbt/assets` (only when `CI_PROJECT_DIR` is set), `<project>/../assets`, `/usr/app/dbt/assets`; none exists: the first candidate. A relative value is relative to the project root. |
| `adbc_driver` | path | `""` | The SingleStore ADBC driver file. `$VDP_ADBC_DRIVER` wins over it; a relative value is relative to the working directory. Empty: `lib/dbt/libadbc_driver_singlestore.so` in a directory above the dbt engine binary or above `vdp` itself, else the system library search path. |

**`[secrets]`** (`SecretsSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `vault_mount` | string | `"kv-other"` | KV v2 mount of the secrets. |
| `precedence` | array of strings | `["vault:vdp/airflow", "dotenv", "vault:vdp/{env}/dbt", "env"]` | Sources, highest priority first: `vault:<path>` (`{env}` and `{env_short}` are replaced), `dotenv`, `env`. An unknown entry is an error. `--no-vault` drops the Vault entries for the run. |
| `vault_required` | bool | `false` | `true`: Vault that cannot be read (or has no AppRole credentials) is an error. `false`: a failure warns, missing credentials skip Vault quietly. |

**`[environments.<name>]`** (`EnvironmentSettings`; the four built-in environments are always defined, other names need a section)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `vault_url` | string | `LDV`, `ABT`, `CBT` and custom: `"https://vault.ops.example.com:8200"`; `PRD`: `"https://vault.example.com:8200"` | Vault base URL. Empty does not switch Vault off: with a `vault:` entry in `[secrets].precedence` and AppRole ids set, an empty URL is a login failure (a warning, and `env doctor` fails). To run without Vault, leave `vault:` entries out of `[secrets].precedence` (the dbt workshop) or pass `--no-vault`. |
| `ca_certificate` | string | `LDV`, `ABT`, `CBT` and custom: `"root_intermediate_nonprod.cert"`; `PRD`: `"root_intermediate_prod.cert"` | File name in the assets directory: the CA for Vault and, as `$VDP_SSL_CA`, for the database's TLS in `profiles.yml`. Empty: no CA; `env doctor` does not look for one. |
| `role_id_vars` | array of strings | `LDV`, `ABT`, `CBT` and custom: `["KEYVAULT_ROLE_ID", "KEYVAULT_ROLE_ID_LDV_ABT"]`; `PRD`: `["KEYVAULT_ROLE_ID", "KEYVAULT_ROLE_ID_CBT_PRD"]` | Variables (dotenv file, then process environment) that hold the AppRole role id; the first one set wins. |
| `secret_id_vars` | array of strings | `LDV`, `ABT`, `CBT` and custom: `["KEYVAULT_SECRET_ID", "KEYVAULT_SECRET_ID_LDV_ABT"]`; `PRD`: `["KEYVAULT_SECRET_ID", "KEYVAULT_SECRET_ID_CBT_PRD"]` | The same for the AppRole secret id. |

**`[dbt]`** (`DbtSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `binary` | path | `""` | The dbt v2 engine. Empty: `$VDP_DBT_BIN`, else the first `dbt` on `PATH` that is not this wrapper. A configured path is used as written, without a check that it exists. |
| `grammar` | string | `"engine"` | `embedded`: use the grammar embedded in `vdp`. Anything else: derive it from the engine, cache it per binary, and fall back to the embedded copy when that fails. |
| `guarded_commands` | array of strings | `["build", "run"]` | Commands refused without a selector, unless a service user runs them. |
| `selector_flags` | array of strings | `["select", "selector"]` | Long names of the flags that count as a selection (`-s` is read as `select`). |
| `user_secret` | string | `"SINGLESTORE_USER"` | Secret holding the SingleStore user name the guard looks at. |
| `service_user_suffix` | string | `"_dbt"` | A service user's name ends with this... |
| `personal_user_prefix` | string | `"pu"` | ...and does not start with this (personal accounts). |
| `offline_commands` | array of strings | `["help", "completions", "man", "init"]` | Commands that run without resolving secrets. `--help`, `--version` and a missing command do too. |
| `missing_source_hint` | bool | `true` | After a failed run, print `vdp sources add ...` for sources dbt reports as undefined (dbt1048). |
| `env` | table of strings | `{ DBT_ALLOW_EXPERIMENTAL_ADAPTERS = "true" }` | Variables set for dbt unless the process environment already has them. A configured table replaces the default whole. |

**`[sources]`** (`SourcesSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `generated_dir` | string | `"models/sources"` | Where the generated `sources_<database>.yml` files live, relative to the dbt project. |
| `databases` | array of strings | `["edw_dv_bdv", "edw_dv_rdv", "edw_fmc", "edw_inc", "information_schema", "tenant_ai", "tenant_leertijd", "vdp_mgmt", "vdp_psa"]` | Databases `sources rewrite` regenerates (physical names). The default is the original organisation's list: set your own. |
| `include_columns` | bool | `true` | Write column names into source tables. |
| `column_order` | string | `"name"` | `ordinal`: columns in table order. Anything else: alphabetical. |
| `database_prefix` | table | not set | See the next table. Without it a source's database is its physical name. |

**`[sources.database_prefix]`** (`DatabasePrefix`; both keys are required)

| Key | Type | Meaning |
|---|---|---|
| `physical` | string | Prefix of the databases `vdp` connects to (`"dev_"`). |
| `jinja` | string | What source files say instead (`"{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_"`). Whitespace and quote style are ignored when reading. |

**`[git]`** (`GitSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `timeout_secs` | integer | `600` | Seconds one git command of the release flows may run before `vdp` stops it (a hung network call or an ssh prompt would block the command for ever). `0`: no limit. The release hub has its own `[release_hub].git_timeout_secs`. |

**`[jira]`** (`JiraSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `base_url` | string | `"https://issuetracker.example.com"` | Jira base URL (REST v2, basic authentication). |
| `user_secret` | string | `"JIRA_USER"` | Secret holding the account. Credentials are never prompted for. |
| `token_secret` | string | `"JIRA_TOKEN"` | Secret holding the API token. |
| `excluded_fix_versions` | array of strings | `["Nog in te plannen"]` | Fix version names that revert a ticket which has no DROP or EDROP fix version. |
| `advance_transition` | string | `""` | Transition `jira advance` takes, matched by name or target status without regard to case. Empty: the first transition Jira offers. |
| `sprint_field` | string | `"customfield_11221"` | Jira custom field that holds the sprints (a field id of the original organisation's Jira). |

**`[release]`** (`ReleaseSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `ticket_prefix` | string | `"INTEL"` | Jira project key in commit messages (`INTEL-1234`); the original organisation's key. |
| `from` | string | `"origin/dev"` | Source of releases. Its local name (`dev`) is the branch that is never written. |
| `to` | string | `"origin/main"` | Where releases go and the base for the merge-base. Its local name (`main`) is written by promotions only. |
| `branch_prefix` | string | `"release/"` | Prefix of release branches; the only branches (besides `main`) the tools may write. |
| `validate` | array of strings | `["compile", "--no-partial-parse"]` | dbt arguments run in the project (with `--profiles-dir` and `--target` added) after the branch is built and before it is pushed. |

**`[qa]`** (`QaSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `run_results_table` | string | `"vdp_mgmt.dbt_run_results"` | `database.table` with the run results of past dbt runs (`unique_id`, `execution_time`, `run_started_at`) for `qa view-candidates`. `{env}` and `{env_short}` are replaced: `{env_short}_vdp_mgmt.dbt_run_results` is `dev_vdp_mgmt.dbt_run_results` for `LDV`. |
<!-- config-tables:end -->

## 5. Architecture

Every capability is a crate behind a trait and, in `vdp-cli`, a cargo feature, so a deployment can pick:

| Crate | Purpose | Key types | Depends on |
|---|---|---|---|
| `vdp-core` | Environments, layered configuration, project discovery, the lazy service context, batch reporting | `Environment`, `Config`, `Context`, `ContextBuilder`, `ProjectLayout`, `Outcome`, `BatchReport` | none |
| `vdp-secrets` | Secrets without the process environment | `SecretProvider`, `ChainProvider`, `DotenvProvider`, `EnvProvider`, `VaultClient`, `child_env` | none |
| `vdp-sql-script` | SQL scripts as text: where statements start and end | `StatementSplitter`, `RoutineAwareSplitter`, `tokenize` | none |
| `vdp-db` | The database port and its SingleStore adapter over the ADBC driver | `SqlSession`, `AdbcSession`, `DriverSource`, `ConnectionSettings`, `TlsMode`, `ResultSet` | secrets, sql-script |
| `vdp-dbt` | Driving dbt v2: grammar, argv scan, guard, subprocess engine, profile, manifest | `DbtEngine`, `SubprocessDbt`, `Grammar`, `Scan`, `DbtSettings`, `DbtProject`, `Manifest` | core, db, secrets |
| `vdp-sources` | dbt source maintenance from the SingleStore catalog | `Catalog`, `SqlCatalog`, `SourceIndex`, `AddRequest`, `add`, `rewrite` | core, db, dbt |
| `vdp-git` | Git through the system `git` client, with a write guard | `Vcs`, `GitCli`, `GitSettings`, `WriteGuard`, `PushMode`, `PushRejection` | core |
| `vdp-jira` | Jira tickets, fix versions, status transitions | `IssueTracker`, `JiraClient`, `FixVersion`, `advance_tickets` | core, secrets |
| `vdp-release` | The DROP release flows | `Analyser`, `ReleaseFlow`, `PatchFlow`, `Promotion`, `BranchRules`, `ReleasePolicy`, `Changelog` | core, git, jira |
| `vdp-sql` | Ad-hoc SQL and CSV export | `quote_table`, `run_file`, `export_table`, `save_csv` | core, db |
| `vdp-qa` | Quality checks on the manifest | `orphan_tables`, `view_candidates`, `QaSettings` | db, dbt |
| `vdp-cli` | Binary `vdp`, the `dbt` multicall, hand-offs; the only composition root; also a library | `Cli`, `GlobalArgs`, `Command`, `bootstrap::context`, `external::HANDOFFS` | core, db, dbt, secrets; sources, git, jira, release, sql, qa behind their features |

| Cargo feature of `vdp-cli` | Crates | Commands |
|---|---|---|
| always | `vdp-core`, `vdp-secrets`, `vdp-db` (with `vdp-sql-script`, which `vdp-cli` reaches through it), `vdp-dbt` | `dbt`, `vdp dbt`, `env`, `dev` |
| `sources` (default) | `vdp-sources` | `sources add`, `sources rewrite` |
| `release` (default) | `vdp-git`, `vdp-jira`, `vdp-release` | `release create\|analyse\|patch\|promote\|branches`, `jira advance` |
| `sql` (default) | `vdp-sql` | `sql run`, `sql export` |
| `qa` (default) | `vdp-qa` | `qa orphan-tables`, `qa view-candidates` |
| none: hand-off | none (the `release-ui` and `vaultspeed-framework` workspaces) | `release-ui ...`, `vaultspeed ...` |

`vdp-release` also has a feature `ts` that derives TypeScript bindings of its API types (for the release UI's front end).

**The composition root** is `crates/vdp-cli/src/bootstrap`. Modules never name each other's concrete types: `bootstrap::context` registers
factories in the `vdp_core::Context`, and a command asks for what it needs (`Arc<dyn SecretProvider>`, `Arc<dyn SqlSession>`,
`Arc<dyn DbtEngine>`, `Arc<dyn Vcs>`, `Arc<dyn IssueTracker>`). Services are created on first use, once, and a failure is remembered; a
command that only needs git never logs in to Vault. A service nobody registered is an error naming it. A command group whose feature was
compiled out does not exist: its variants are gated away in `args.rs`, so `vdp sql ...` is parsed as an unknown subcommand and exits 2 with
clap's "unrecognized subcommand". Every module crate is also a plain library.

**`vdp-cli` as a library.** The sibling binaries build their command lines on it: `args::GlobalArgs` and `args::Command`,
`bootstrap::context`, `bootstrap::adbc_driver`, `bootstrap::resolve_secrets_from` with `SecretSources` and `ResolvedSecrets`,
`commands::run_command` for every command the other binary does not handle itself, `commands::run_dbt_validation` (the dbt validation of
releases, which the release hub reuses), and `runtime` (logging and exit codes). They depend on it with `default-features = false` and
enable the module features they need.

## 6. The dbt wrapper and sources

### The dbt wrapper

**Argument fidelity is the first rule: every argv that dbt v2 accepts reaches dbt with the same meaning.**

- **Byte-for-byte forwarding.** Tokens go through unchanged as `OsString`s, including non-UTF-8 ones. The only values ever changed are
  `--args`/`--vars` values dbt would misread or reject:
  - `--vars` is read by dbt with `dbt-yaml` (the same version, `0.9.6`, and the `yaml_11` feature as the dbt fork uses; this workspace pins it with `=`). dbt already recovers
    `{a:1}` itself, so `--vars` is only touched when dbt would *reject* it and the old Python wrapper's repair (a space after `:`, now only
    outside quotes) makes it valid: `{a:{b: 1}}`.
  - `run-operation --args` gets dbt's own `--vars` recovery, because dbt silently reads `{days:7}` as `{"days:7": null}` there.
  - A changed value is re-emitted as JSON whose round trip through dbt's reader is checked first. Each change is logged as a warning.
- **The guard sees the command as dbt sees it.** The argv is scanned with dbt's own grammar: every flag of every (sub)command, whether it
  takes one value or several, and hidden aliases. That grammar is derived from the installed engine (`dbt completions fish` plus
  `<command> --help`) and cached per binary. A copy is embedded (`crates/vdp-dbt/grammar/dbt.json`, refreshed with `vdp dev grammar`).
  `dbt --debug build` and `dbt --target x build` are therefore guarded, `--select a b build` makes `build` a selector value (as in clap),
  and `--selector` counts as a selection.
- **The guard rule.** `build`/`run` without `-s/--select/--selector` is refused (exit 1), unless the SingleStore user is a service user
  (a name ending in `_dbt` and not starting with `pu`; `[dbt].user_secret` names the secret). `--help` and `--version` are never refused.
- **Transparent process.** dbt shares the terminal. The wrapper survives Ctrl-C (the terminal signals dbt as well) and forwards SIGTERM,
  SIGHUP and SIGQUIT. The exit code passes through, and a death by signal maps to 128+n.
- **Environment for dbt.** dbt also gets `DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true` (needed for SingleStore in dbt v2; `[dbt].env`, unless
  already set), `VDP_ENV` and `VDP_SSL_CA` (the environment's CA, for `ssl_ca:` in `profiles.yml`), and every key of the dotenv file and
  Vault. `help`, `completions`, `man`, `init`, `--help`, `--version` and a missing command run without resolving secrets.
- **Hint for undefined sources.** After a failed run with undefined sources (dbt1048), it prints `hint: ... vdp sources add <source>.<table>`
  (from the new part of `dbt.log`: `--log-path`, else `$DBT_LOG_PATH`, else `<project>/logs`).
- **Project and environment.** dbt's own `--project-dir` selects the project whose `vdp.toml`, dotenv file and secrets apply. The context
  is built before anything runs, so an unknown `$DBT_DEPLOY_TARGET` or an invalid `vdp.toml` stops even `dbt --version` (exit 2).

dbt v2 itself (not this toolkit): it reads `--vars '{a: yes, c: 007}'` as the strings `"yes"` and `"007"` (dbt-core v1 read `true` and
`7`); `profiles.yml` needs `type: singlestore` with the v2 fields (`ssl_mode`, `ssl_ca: "{{ env_var('VDP_SSL_CA') }}"`).

### Sources

`vdp sources add --missing` runs `dbt parse --log-format json` and collects the dbt1048 `Source '<source>.<table>' not found` events. Each
table is then looked up in `information_schema`, with bound parameters. Where the table goes depends on how its source is declared:
- **Hand-written YAML file** (with anchors, comments, any indentation): the table is inserted as text after the source's last table, in the
  file's own indentation. The edited file is parsed again and must equal the old one plus exactly the new table, or nothing is written.
- **Not declared yet:** it goes to `models/sources/sources_<db>.yml` (`[sources].generated_dir`), written in the layout of the Python
  `DbtSourceDumper`. These generated files keep the descriptions, tests and meta people added; the Python code dropped them.
- **Table not in the database:** reported with close matches, nothing written.

Afterwards it runs `dbt parse` again and confirms nothing is undefined any more.

**Databases named per environment** (`dev_landing`, `val_landing`, `prd_landing`): `[sources.database_prefix]` maps the databases vdp reads
to what the source files say. `add` and `rewrite` both use it.

```toml
[sources.database_prefix]
physical = "dev_"                                       # the databases vdp connects to
jinja = "{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_"    # what the source files say instead
```

- `vdp sources add dev_landing.t` writes the source `landing` to `models/sources/sources_landing.yml`, with database and schema
  `{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_landing`.
- `vdp sources rewrite` writes the same file for `dev_landing` in `[sources].databases`.
- A hand-written source whose database is that expression (whitespace and quote style do not matter) is read from `dev_landing`, so
  `--missing` can add its tables. Without the option, such a source is left to the developer.

## 7. Differences from vdp-python-utils

Each difference is intentional. The test that pins it is named in the last column. A name marked † is a test of the original repository
that is not in this tree (section 8).

| Before (`vdp-python-utils`) | Now | Test |
|---|---|---|
| The guard read only `argv[0]` (`dbt --debug build` ran everything); `--selector` did not count | dbt's grammar; `--selector` counts | `guard::tests`, `golden_wrapper.rs`†, `dbt_differential.rs`† |
| `--vars`/`--args` were pulled out, re-dumped and moved to the end. The colon repair applied only when *every* key was broken, and never inside nested maps | Tokens stay in place and byte-identical; dbt's reader decides; only values dbt misreads or rejects are fixed | `yaml_arg::tests`, `argv::tests`, `golden_wrapper.rs`† |
| Invalid `--vars`: the Python wrapper exited 1 itself | dbt reports it (exit 2) | `golden_wrapper.rs`† |
| `log(level="ERROR")` raised, so cleanup and exit paths never ran | typed errors, exit 0/1/2 | throughout; `runtime::tests` |
| Vault `verify=False`; secrets exported to `os.environ` | CA-verified; child processes only | `engine::tests`, `wrapper_process.rs`† |
| `dbt` run from `models/` crashed (`Repo(cwd)`) | project found by walking up | `project::tests` |
| Source files lost descriptions, tests and meta on rewrite; `databases=None` crashed | metadata kept; databases from config | `generated::tests` |
| Hand-written source files could not be extended | span-preserving insertion | `editor::tests`, `live_sources.rs`† |
| SQL built by string formatting (`download_from_singlestore`, introspection) | bound parameters, quoted identifiers | `vdp-sql` unit tests, `catalog.rs` test† |
| SQLAlchemy/mysqlclient (Python), the `mysql` crate (the VaultSpeed framework) | every connection goes through SingleStore's ADBC driver, the one dbt v2 uses (found like dbt finds it); profile TLS settings map as in dbt v2; a script returns its last result set | `vdp-db` unit tests, `tests/live_adbc.rs`† |
| Release: EDROP fix versions sorted after DROPs and compared with the target | any EDROP fix version keeps the ticket, in releases and patches (EDROP numbers run on their own) | `policy::tests` |
| Patch: cherry-picked only the feature tip, so multi-commit features lost commits | the ticket's mainline merges with `-m 1` (the whole feature); a ticket reverted on the branch is re-enabled by reverting the revert | `scenario.rs`† |
| Revert conflicts (a file changed on one side, deleted on the other) stopped the release; cherry-picks deleted the file | the CLI resolves leftover conflicts for the change being applied (`-X theirs` semantics) and names the files; a caller with a resolver (the release UI) pauses and lets the release manager decide per file (or take a suggested selection change); `strict` surfaces every hunk | `repo.rs` (`the_default_conflict_handling_resolves_for_the_change_and_names_the_paths`), `real_repo.rs`†, `scenario.rs`†, `api.rs`† |
| Promotion pushed main with `--force`; release branches sorted as strings | `--force-with-lease` bound to the head the preview saw (a push in between is refused, never overwritten); sorted by fix version (numerically) | `scenario.rs`† (lease) |
| Nothing stopped a release branch named `dev`, a patch of `dev`, or a revert with HEAD on `dev` | `dev` is never written: branch names are validated (`<branch_prefix>*` only), and `GitCli` refuses writes and pushes outside `release/*` and `main`, and on a detached HEAD; a hub's clone keeps no local `dev`. Reverts found on `dev` (GitLab revert MRs, back-merges of `release/*` or `main`) are reported, never repeated | `rules::tests` (`only_release_branches_are_targets`, `the_guard_protects_dev`), `nothing_ever_writes_dev`†, `real_repo.rs`† |
| Any message containing "revert" was a revert; a re-enable (`Revert "Revert …"`) counted as a revert, so the next patch removed the feature again | a revert graph: git's `This reverts commit` trailer, else the `Revert "`/`Reapply "` depth; per ticket the net state on the compare branch | `scan::tests`, `a_re_enabled_ticket_stays_enabled`† |
| A GitLab "Revert" MR merge counted as a second merge of the ticket; merges without a ticket (`Merge branch 'fusion'`) were invisible and always shipped | revert MRs and back-merges are classified by what they bring; ticketless merges are `branch:<name>` pseudo-tickets (kept by default, revertable) | `scan::tests` |
| A revert on main of a ticket without a fix version was not carried over | carried over while the ticket is still out by policy (`keep_empty` decides) | `a_revert_of_a_ticket_without_fix_version_is_carried_over`† |
| Promotion kind from main's last commit message; promoting an older release rolled main back silently; recreate and new promotion dropped commits silently | detection by ancestry (nothing to promote, patch, new); a rollback needs `--accept-rollback`; dropped commits are listed and need `--accept-losses` | `promotion_is_detected_by_ancestry_and_guarded`†, `recreating_a_release_shows_what_it_drops_and_binds_to_the_preview`† |
| Any release branch could be changed | only the newest two (`--allow-old-release` in the CLI); a promoted branch is patched, not recreated | `only_the_newest_two_release_branches_can_change`† |
| No record on the branch of what a release contains | `changelog/<DROP>.md` (accepted, added, excluded, patches; JSON front matter) is the branch's last commit; a patch rewrites it at the tip; releases carry the others forward from main | `changelog::tests`, `the_changelog_is_the_last_commit_and_a_patch_rewrites_it`† |
| Push failures were raw git text; git could hang forever | `PushRejection` (non-fast-forward, stale lease, protected, hook, auth, network), verified by a fetch; `GitCli::with_timeout` stops a hung git call (`[git].timeout_secs` for the CLI, `[release_hub].git_timeout_secs` for the hub) | `real_repo.rs`† |
| A commit inside a merged feature naming another ticket became a ticket of its own (its commit reverted separately) | a mainline merge belongs to the first ticket in its message (the branch's); the others are `also_mentions` | `scan::tests` (`other_ids_in_a_merge_are_also_mentions`), parity run† |
| Merges scanned newest-first, so a revert was seen before its merge and the reverted ticket stayed in scope; feature commits were counted besides merges; ticket ids cut to 11 characters | mainline only, oldest first; whole ids | `scan::tests`, `scenario.rs`† |
| Fix versions compared as strings (`DROP5` > `DROP10`); "Nog in te plannen" never matched, so `--keep-empty-fixversions` kept it | numeric drop number; exclusions apply | `fix_version::tests`, `policy::tests` |
| A Jira lookup failure counted as "no fix version" and silently reverted the ticket | `release create` stops and names the tickets; an analysis (and a UI board) marks them per ticket | `scenario.rs`† |
| Release branch pushed right after creation, before `dbt compile` | pushed only after validation; dry run changes nothing local | `scenario.rs`† |
| Work tree auto-stashed silently | refused when dirty | `flow.rs`† |
| Jira transition by list index; credentials prompted with `input()` | by name (default: the first, as before); secrets only | `vdp-jira` advance tests, `jira_advance.rs` |
| QA orphans counted tests as readers | tests are not readers | `orphans_ignore_tests`, `golden_qa.rs`† |

† Pinned by a test (or a one-off comparison run) of the original repository, not in this tree.

## 8. Testing

**What exists** (from `cargo test --workspace --locked`, default features): 282 tests, none ignored.
- Unit tests, 201, in every crate: `vdp-core` 35, `vdp-dbt` 29, `vdp-cli` 27, `vdp-git` 22, `vdp-release` 21, `vdp-sources` 15, `vdp-db` 14, `vdp-jira` 13, `vdp-sql-script` 10, `vdp-secrets` 9, `vdp-qa` 4, `vdp-sql` 2.
- Integration tests, 61: `crates/vdp-cli/tests/` (`config_reference.rs` 3, `doctor.rs` 1, `dry_run.rs` 2, `environment_aliases.rs` 3, `external.rs` 10, `git_timeout.rs` 2, `jira_advance.rs` 3, `log_json.rs` 11, `no_script_names.rs` 2, `no_vault.rs` 4: the built `vdp` as a process), `crates/vdp-git/tests/repo.rs` 9 (real throw-away git repositories),
  `crates/vdp-jira/tests/http_contract.rs` 11 (a local HTTP server in place of Jira).
- Doc tests, 20: the crate-level example of each of the 12 crates' `lib.rs`, a second example on `vdp-git`, `vdp-sources` (both `no_run`) and
  `vdp_secrets::SecretProvider`, and five in `vdp-core` (`batch.rs` 3, `present.rs` 2). The examples of `vdp-db`, `vdp-git` and
  `vdp-sources` that touch a database or git are compile-only.
- Fakes are hand-written (no mocking crates): fake Vault and Jira servers, fake catalog, SQL session, dbt engine and tracker.

**How to run.** In Docker, from the repository root (no Rust toolchain on the host; see "The quality gate" below):

```bash
make rust-test W=vdp-rust-utils                                  # everything above
make rust-test W=vdp-rust-utils S="-p vdp-dbt"                   # one crate
make rust-test W=vdp-rust-utils S="-p vdp-cli --test external"   # one integration test file
```

Or with a host `cargo`, in this folder, with the pinned toolchain:

```bash
cargo test --workspace --locked                       # everything above
cargo test -p vdp-dbt                                 # one crate
cargo test -p vdp-cli --test external                 # one integration test file
```

**The quality gate** before a change is done. In Docker, from the repository root:

```bash
make rust-check W=vdp-rust-utils    # fmt, clippy (all features, then no features and each one alone), tests, docs
make rust-check                     # the same for all three workspaces: use it after a public API change here
```

`make rust-check` runs the image `workshop-rust-check` (`infrastructure/rust-check/`, `rust:1.96-trixie` with the version taken from
`ARG RUST_VERSION` of the dev image `Dockerfile`) as the host user; the cargo registry and `target/` live in Docker volumes (`make rust-clean`
removes them), so the sources stay clean. `CARGO_BUILD_JOBS=<n> make rust-check` limits the parallelism; `make rust-cargo W=vdp-rust-utils
S="<cargo arguments>"` runs any other cargo command. The same gate with a host `cargo`, in this folder:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings     # all + pedantic (see [workspace.lints])
cargo test --workspace --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --all-features
cargo clippy -p vdp-cli --all-targets --no-default-features -- -D warnings   # and once per feature: sources, release, sql, qa
(cd ../release-ui && cargo build --locked --workspace)                       # the siblings use this workspace by path
(cd ../vaultspeed-framework && cargo build --locked --workspace)
```

**Not covered in this tree.** The cleaned copy dropped: the wrapper's process tests, the differential suite against a real dbt binary and the
grammar-drift test (so nothing checks the embedded grammar or the argv handling against the engine: after a change in `argv/`, `flags/`,
`guard.rs` or `wrapper.rs`, try `dbt --version`, `dbt build -s x`, `dbt --select a b build` and `dbt run-operation m --args '{a:1}'` by hand);
the golden parity fixtures against the Python code; the release scenario tests on real repositories; the opt-in live suites (a dbt project on
a running SingleStore; the `SqlSession` contract against the real ADBC driver). Never exercised here: a Vault login, Jira, GitLab, pushes to a
real remote, a database connection. Details: `.agents/skills/vdp-rust-utils/references/testing.md`.

## 9. Known gaps

Left as they are; fix them deliberately.
- The `vdp dev grammar` help example, comments and tests cite a development-machine checkout of the dbt fork instead of the base image's submodule.
- The wrapper's process, differential and grammar-drift tests, the live suites and the golden fixtures are not in this tree (section 8); only
  29 `vdp-dbt` unit tests and the doc tests guard the wrapper. `vdp-release` has no scenario tests.
- `[sources].databases`, `[release].ticket_prefix`, `[jira].sprint_field` and the Vault URLs and CA file names default to the original
  organisation's values (placeholders for hosts). Set them in `vdp.toml`.
- `vdp release create --help` says a fix version in `[jira].excluded_fix_versions` reverts the ticket; `ReleasePolicy::classify` applies it only
  to a ticket that has no DROP or EDROP fix version.
- A test comment in `crates/vdp-cli/tests/external.rs` still says a full build leaves `vdp-vaultspeed` and `vdp-release-ui` beside `vdp`; those
  crates are in the sibling workspaces now.
