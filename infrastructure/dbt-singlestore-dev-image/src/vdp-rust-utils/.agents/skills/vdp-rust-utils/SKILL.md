---
name: vdp-rust-utils
description: >-
  The vdp-rust-utils Cargo workspace (infrastructure/dbt-singlestore-dev-image/src/vdp-rust-utils): the `vdp` toolkit and the `dbt`
  wrapper for dbt v2 on SingleStore, with its crates (vdp-cli, vdp-core, vdp-secrets, vdp-db, vdp-dbt, vdp-sources, vdp-git,
  vdp-jira, vdp-release, vdp-sql, vdp-sql-script, vdp-qa), the layered vdp.toml configuration, the dbt argument grammar and
  selection guard, source maintenance, DROP release flows, and how the binary is built into the dev image, used by the devcontainer
  and Workshop Studio, and shared by path with the release-ui and vaultspeed-framework workspaces. Use it when you read, change,
  build, test or document anything in that workspace: a vdp command or flag, a config key or section, the dbt wrapper (argv, guard,
  grammar, `dbt` link), a crate or cargo feature, a hand-off binary, the `vdp-build` stage of the dev image Dockerfile, the Rust
  toolchain version, or when `vdp` or `dbt` behaves unexpectedly in the devcontainer.
---

# vdp-rust-utils

## 1. When to use
Load it before touching `src/vdp-rust-utils/` (below: the workspace, `W`). `W/README.md` is the user-facing manual (commands, config,
architecture, differences from the old Python tools, known gaps); this skill is the working guide. Depth is in `references/`.
`W` is a cleaned copy (docs, fixtures, sample config, golden, differential and live tests removed); untouched originals are kept
locally in `infrastructure/dbt-singlestore-dev-image/local/` and describe a larger code base. Never copy organisation hostnames, e-mail
addresses or personal paths from them.

## 2. Map
| Crate (`W/crates/`) | Responsibility | Start at |
|---|---|---|
| `vdp-cli` | Binary `vdp` (multicall: argv0 `dbt` is the wrapper), the only composition root, commands, hand-offs; also a library the siblings reuse | `src/main.rs`, `src/bootstrap/mod.rs`, `src/args.rs` |
| `vdp-core` | `Environment`, layered `Config`, `Context` (lazy typed services), `ProjectLayout`, `Outcome`, `BatchReport` | `src/config.rs`, `src/context.rs` |
| `vdp-secrets` | `SecretProvider`: env, dotenv, Vault AppRole, chain; `child_env` | `src/lib.rs` |
| `vdp-db`, `vdp-sql-script` | `SqlSession` over SingleStore's ADBC driver; SQL statement splitter | `vdp-db/src/session.rs`, `adbc_session.rs` |
| `vdp-dbt` | Engine, grammar (`grammar/dbt.json`), argv scan, guard, `--args`/`--vars` fixes, profile, manifest, dbt1048 | `src/wrapper.rs`, `src/argv/` |
| `vdp-sources` | `sources add` / `--missing` / `rewrite`, span-preserving YAML editor | `src/add.rs`, `src/editor.rs` |
| `vdp-git`, `vdp-jira`, `vdp-release` | Git port and CLI adapter with write guard; Jira port and client; release, patch, promotion flows | `vdp-release/src/flow.rs`, `rules.rs` |
| `vdp-sql`, `vdp-qa` | `sql run` / `export`; `qa orphan-tables` / `view-candidates` | `src/lib.rs` |

Per-crate types, traits and invariants: `references/modules.md`.

## 3. How it fits the stack
| Aspect | Fact |
|---|---|
| Build | `cargo build --release --locked -p vdp-cli` (pinned toolchain 1.96); features `sources`, `release`, `sql`, `qa` are all default |
| Ship | Dev image stage `vdp-build` in `infrastructure/dbt-singlestore-dev-image/Dockerfile`; a prebuilt `./vdp` in that folder wins; neither: image without vdp |
| Install | `/usr/local/bin/vdp`; `dbt` there is a link to `vdp`; the real engine is `/usr/local/libexec/dbt/dbt` (`VDP_DBT_BIN`); the ADBC driver stays in `/usr/local/lib/dbt/` |
| Run | Devcontainer (`dbt-singlestore-workshop/.devcontainer`, image `dbt-singlestore-dev:1.11.11`), config `dbt-singlestore-workshop/vdp.toml`, `vdp:` tasks in `.vscode/tasks.json`, pre-commit and `tests/setup_tests.sh` call `dbt` (the wrapper) |
| Workshop Studio | `actions/images.py` runs `vdp --version` (must match `\d+\.\d+\.\d+\S*`); `actions/dbt.py` runs `dbt` as developer users; `actions/toolbox.py` (`toolbox.run`) runs shell commands in the dev image as a developer or the robot and counts a refusal only when an output line starts with `vdp:`; `scenarios/same-toolbox.yaml` quotes the Dockerfile between `RUN --mount=type=bind,source=src/sqlfmt` and `ln -s` |
| Siblings | `src/release-ui` and `src/vaultspeed-framework` depend on these crates by path (`../vdp-rust-utils/crates/<name>`) and are built by hand; `vdp vaultspeed ...` / `vdp release-ui ...` exec their binaries (exit 2 when absent, as in the image) |
| Not consumers | `.gitlab-ci.yml`, the project `Dockerfile` (`dbt-vdp:<tag>`) and the Airflow DAGs use plain `dbt`, never `vdp` |

Contracts to keep: `vdp --version` format; the `vdp.toml` keys the workshop uses; the `vdp:` task flags; the Dockerfile anchors; the
layout `libexec/dbt/dbt` + `lib/dbt/`; the public API of `vdp-cli` and the module crates (siblings). Detail: `references/image-and-consumers.md`.

## 4. Rules that are easy to break
- **`dev` is never written**, not even locally. `BranchRules` (`vdp-release/src/rules.rs`) validates targets and `GitCli`'s `WriteGuard`
  enforces it at the git layer; keep both in every new release code path (`vdp-cli/src/bootstrap/mod.rs` builds the guard).
- **Argument fidelity of the `dbt` wrapper.** Every argv dbt accepts reaches dbt unchanged. Never rewrite argv tokens except the
  `--args`/`--vars` fixes in `vdp-dbt/src/argv/yaml_arg.rs`; the guard decides from the scan and never edits. The dbt differential suite
  is gone, so check by hand after changes in `argv/`, `flags/`, `guard.rs`, `wrapper.rs` (`testing.md`).
- **Composition root only in `vdp-cli/src/bootstrap`.** Modules depend on `vdp-core` and talk to each other through traits
  (`SqlSession`, `DbtEngine`, `Vcs`, `IssueTracker`, `SecretProvider`, `Catalog`); never name a concrete adapter in a module crate.
- **Every module is a cargo feature of `vdp-cli`.** Gate its args, commands and providers with `#[cfg(feature = "...")]`; each feature
  must build alone (`--no-default-features --features <name>`).
- **A config change touches the settings struct, its `Default` and `commands::MODULE_SECTIONS`** (new sections), and by hand the key
  tables in `README.md` and `references/config.md`; `vdp-cli/tests/config_reference.rs` compares them with the structs. Settings structs are
  `#[serde(deny_unknown_fields, default)]` with `const SECTION`.
- **Secrets never enter the process environment.** Only child processes get them (`vdp_secrets::child_env`); `Debug` impls redact.
- **Help text is tested:** every command has `about`, `long_about`, an `Examples:` block (leaves), help on every argument, lines
  within 100 columns (`args.rs` tests). Wrap `[section]` and `<placeholder>` in backticks (help text is also rustdoc).
- **Exit codes:** 0 success; 1 the command ran but its work failed (a batch with failures, a failed check) or dbt's own code (the guard's
  refusal of an unselected `dbt build`/`run` is also 1); 2 could not start (bad arguments, configuration, unknown environment,
  unreachable service, refused operation). Commands return
  `Result<Outcome>`; an `Err` is printed as `vdp: error: ...` and exits 2.
- **Style:** edition 2024, `unsafe_code` forbidden, clippy `all` + `pedantic` clean, `thiserror` in libraries, `anyhow` only in
  `vdp-cli`, hand-written fakes (no mocking crates), `# Errors` docs on public `Result` functions.
- **The public API is shared with the siblings by path.** A change to a public item of `vdp-cli` or a module crate must be checked by
  building `release-ui` and `vaultspeed-framework`.
- **Intentional behaviour changes** (anything the Python tools did differently) get a row in the README's differences table.

## 5. Playbooks
**Add a command to an existing module**
1. `args.rs`: variant in the module's subcommand enum under its `#[cfg(feature)]`, with doc comments; long texts as constants in `help.rs`
   (`long_about`, `after_long_help` with `Examples:`, one `long_help` per argument; wrap by hand within the limits at the top of `help.rs`).
2. The logic goes in the module crate (for `sql`: `vdp-sql`), not in `vdp-cli`. In `commands/<group>.rs` add the arm to the group's
   function (for example `sql::sql` matches `SqlCommand`); it reads `ctx.config().section::<Settings>(...)` and
   `ctx.service::<Arc<dyn ...>>()`, calls the crate and prints, returning an `Outcome` (1 for partial failure via
   `BatchReport::outcome()`), or `Err` for setup problems. `commands::dispatch` has only one arm per group: touch it for a new group.
3. Tests: unit test with a fake port, a process test in `crates/vdp-cli/tests/` if output or exit codes matter; README command reference; a
   `vdp:` task in `dbt-singlestore-workshop/.vscode/tasks.json` only if the workshop should offer it.

**Add a config key** - field and `Default` in the settings struct; use it; update both key tables (`README.md`, `references/config.md`,
identical text; `config_reference.rs` checks them, and wants a line in its `from_code()` for a new settings type); if the workshop's `vdp.toml` should set it, edit that file; run `cargo test -p <crate>`.

**Add a module crate** - `crates/vdp-<name>` with the manifest shape of a sibling crate (`version.workspace = true`, `[lints] workspace = true`);
entry in `[workspace.dependencies]`; depend on `vdp-core` and port crates only; `thiserror` error enum; `<Name>Settings` with `SECTION`;
in `vdp-cli/Cargo.toml` an optional dependency and `<name> = ["dep:vdp-<name>"]` (add to `default` unless opt-in); gate args, commands
(`mod` and `dispatch` arm) and any provider in `bootstrap` with `cfg`; add the section to `MODULE_SECTIONS`; extend the crate table in
the README, `CLAUDE.md`, the map above and `references/modules.md`; if a sibling should offer it, add the feature to its `vdp-cli` line.

**Add a hand-off binary** - entry in `external::HANDOFFS` (`command`, `binary`, `keeps_command`, `what`, `workspace`); a case in
`crates/vdp-cli/tests/external.rs` with a stand-in binary; the "Command groups" and "Other binaries" text in `help.rs` (`ROOT_AFTER_LONG_HELP`,
`ROOT_AFTER_HELP`); its config sections in `MODULE_SECTIONS`; in the Dockerfile a stage next to `vdp-build` (or a `COPY` of a prebuilt file) and
an `install` next to `vdp` in `/usr/local/bin`, outside the quoted Studio anchors; then the Studio catalog check.

**Refresh the dbt grammar** (after the dbt engine in the base image changes) - run
`vdp dev grammar --dbt <dbt v2 binary> --out crates/vdp-dbt/grammar/dbt.json` (it runs `--version`, `completions fish` and `<scope> --help`);
review the diff; compare `HIDDEN_ALIASES` in `vdp-dbt/src/flags/mod.rs` with the `alias`/`aliases` attributes in
`infrastructure/dbt-singlestore-base-image/src/dbt/crates/dbt-clap-core/src/lib.rs`; `cargo test -p vdp-dbt`; `vdp env doctor` should report
"matches the embedded copy". There is no drift test in this tree.

**Change the dev image stage** - edit `Dockerfile` keeping the `libexec/dbt/dbt` + `lib/dbt/` layout, `VDP_DBT_BIN` and the prebuilt-wins logic;
build with `docker build -t dbt-singlestore-dev:1.11.11 infrastructure/dbt-singlestore-dev-image`; then `vdp --version`, `vdp env doctor` in
the image and `docker exec workshop-studio python -m studio.catalog --check`. A prebuilt `./vdp` in that folder (made with
`cargo build --release --locked -p vdp-cli` and `cp target/release/vdp ../../vdp`) silently wins over source edits: refresh or delete it. A running
devcontainer keeps its old image until it is rebuilt or recreated.

**Bump the Rust toolchain** - `rust-toolchain.toml` and `rust-version` in all three workspaces, `ARG RUST_VERSION` in the Dockerfile, the dbt fork's
own channel, the docs that name 1.96 (list in `references/image-and-consumers.md`); gate, siblings, image.

## 6. Verify
Details and the unit-test inventory: `references/testing.md`. Prefer the Docker targets of the root `Makefile`: the host needs no Rust
toolchain, and the registry and `target/` live in Docker volumes, not next to the sources. From the repository root, always:
```bash
make rust-check W=vdp-rust-utils     # fmt, clippy (all features; no features and each of sources, release, sql, qa alone), tests, docs
make rust-test W=vdp-rust-utils S="-p vdp-dbt"    # a quick loop: only tests (S= goes to cargo test)
make rust-check                      # only when the change calls for it: all three workspaces (public API change, path dependencies)
```
`make rust-cargo W=<workspace> S="<cargo arguments>"` runs any other cargo command in the same container (`make rust-clean` removes its volumes).
(The Make variable `W=` names a workspace; it is not this skill's `W`.) With a host `cargo` the same checks run in the workspace folder (the
pinned toolchain installs itself through `rustup`), always:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --all-features
cargo clippy -p vdp-cli --all-targets --no-default-features -- -D warnings   # and once per feature: sources, release, sql, qa
```
Only when the change calls for it:
```bash
(cd ../release-ui && cargo build --locked --workspace)                       # after a public API change: path dependencies
(cd ../vaultspeed-framework && cargo build --locked --workspace)
cargo test -p vdp-release --features ts                                      # only when touching the ts-rs API types
```
Stack level, only when the Dockerfile, `devcontainer.json` or the install layout changed: `docker run --rm dbt-singlestore-dev:1.11.11 vdp --version`,
`... vdp env doctor` (with `-e VDP_NO_VAULT=1`), and `docker exec workshop-studio python -m studio.catalog --check`.
Read-only checks of a build: `vdp --help`, `vdp env show --no-vault` in a scratch directory.

## 7. Known gaps and decisions
Gaps (documented in the README, not fixed): the `vdp dev grammar` help example and some comments cite a development-machine checkout of the dbt fork; the wrapper process, differential and grammar-drift
tests, the live suites and the golden fixtures are removed (29 unit tests remain in `vdp-dbt`, 282 tests in all); `[sources].databases`,
`[release].ticket_prefix` and `[jira].sprint_field` default to the original organisation's values.
Decisions to confirm before changing: `dev` is never written; an EDROP fix version always keeps a ticket; only the newest two release
branches change; `CBT` uses the non-production Vault, CA and AppRole defaults; every database connection goes through the ADBC driver.

## References (load on demand)
- `references/modules.md` - read when working inside one crate: key types, traits, invariants, flows.
- `references/config.md` - read when adding, changing or documenting a config key, or debugging how settings layer.
- `references/testing.md` - read when adding tests, running the gate, or asking what is and is not covered.
- `references/image-and-consumers.md` - read when touching the Dockerfile stage, the workshop files that call `vdp`/`dbt`, the siblings, or the toolchain.
