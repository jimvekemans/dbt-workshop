# vaultspeed-framework

Cargo workspace (nine crates, Rust 1.96) with the VaultSpeed Automation Framework's deployer, binary `vdp-vaultspeed`:
it deploys PML scripts and VaultSpeed generation zips to SingleStore and Airflow, keeps a ledger of what ran and a
local DuckDB history of every object it changed, and adds the VaultSpeed releases to the release hub. **Standalone:**
built into no image and not used by the workshop's compose, nginx, overview or CI, nor by any stack target of its Makefile (only the
Rust checks, `make rust-check W=vaultspeed-framework`, reach it). It needs `../vdp-rust-utils`
and `../release-ui` side by side (path dependencies). The repository's root `CLAUDE.md` rules still apply.

Read the skill before working here: [.agents/skills/vdp-vaultspeed-framework/SKILL.md](.agents/skills/vdp-vaultspeed-framework/SKILL.md)
(loaded as `.claude/skills/vdp-vaultspeed-framework`). The long reference is [README.md](README.md).

## Where things are

| What | Where |
|---|---|
| command line, help texts | `crates/vdp-vaultspeed-cli/src/{args,help}.rs`, `commands/{vaultspeed,history}.rs`, `hub.rs` |
| engine, planner, executor, ledger, sources, reports | `crates/vdp-vaultspeed/src/` (`engine/`, `planner/`, `executor/`, `ledger/`, `sources/`, `report/`) |
| the one place that picks implementations | `crates/vdp-vaultspeed/src/wiring.rs` |
| settings `[vaultspeed]` | `crates/vdp-vaultspeed/src/config/model.rs` |
| pure model (no IO) | `crates/vdp-vaultspeed-domain/` |
| SQL rewrite, classify, fingerprint | `crates/vdp-vaultspeed-sql/` |
| DAG alteration and publication | `crates/vdp-vaultspeed-dags/`, `crates/vdp-ssh/` |
| object history (DuckDB) | `crates/vdp-vaultspeed-history/` |
| release rules and flows; hub extension | `crates/vdp-vaultspeed-release/`, `crates/vdp-vaultspeed-hub/` |

## Commands (the first build compiles DuckDB and tree-sitter and takes long)

Preferred, from the repository root: in Docker, so the host needs no Rust toolchain (pinned 1.96, version read from the dev image `Dockerfile`).
The registry and the build output live in Docker volumes, so no 13 GB `target/` appears here.

```
make rust-check W=vaultspeed-framework                       # the whole gate: fmt, clippy (all features), tests, docs
make rust-test W=vaultspeed-framework S="-p vdp-vaultspeed"  # only tests (S= goes to cargo test)
make rust-check                                              # all three workspaces: after a change in vdp-rust-utils or release-ui
make rust-cargo W=vaultspeed-framework S="build --release --locked -p vdp-vaultspeed-cli"   # any other cargo command
```

With a host `cargo` (rustup installs the pinned toolchain itself) from this folder:

```
cargo build --release --locked -p vdp-vaultspeed-cli      # target/release/vdp-vaultspeed
cargo fmt --check && cargo clippy --workspace --all-targets && cargo test --workspace
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps
cargo test -p vdp-vaultspeed-hub --features ts export_bindings     # TypeScript bindings (VDP_TS_OUT=<file>)
```

## Rules that are easy to break

- The ledger decides what runs; the history only records. Offline (`--offline`, hub plan previews) touches no Vault,
  database or Airflow; a preview writes only reports.
- `wiring.rs` is the only composition of implementations; CLI and hub both use it so they plan identically.
- `raw_fp` drives decisions. Do not change `normalize`, the splitter or classification without treating every existing
  ledger as at risk.
- A refused group is a failed group; a run with failures is `Outcome::Failures` (exit 1), a setup error exits 2.
- Branch model: `main` is the neutral skeleton, environment branches `LDV` `ABT` `CBT` `PRD` sit on top, nothing flows back
  or sideways, `release/*` zip branches are never written. Release management has no CLI, only the hub.
- Help text is tested (about, long_about, `Examples:`, 100 columns); a config key is three edits (struct, `Default`, test).
- No code is copied between this workspace, `vdp-rust-utils` and `release-ui`. A change to their public API means
  `cargo build --workspace --locked` and `cargo test --workspace --locked` here too (`cargo check` is not enough).
- The SQL ledger is one table per SingleStore server, not per environment; the depth is in
  `.agents/skills/vdp-vaultspeed-framework/references/` (`engine.md`, `commands.md`, `config.md`, `release-model.md`).
- Leave `target/` out of searches: it measured about 13 GB after a debug build plus a test build (the debug binary alone is
  1.1 GB; a release build is smaller but slow).
