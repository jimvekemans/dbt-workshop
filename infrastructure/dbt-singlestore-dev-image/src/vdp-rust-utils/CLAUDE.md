# vdp-rust-utils: context for Claude

The Cargo workspace of `vdp`, the toolkit for dbt v2 on SingleStore. One binary, `vdp`; installed as `dbt` it is the dbt wrapper
(argument-faithful, with a selection guard and the secrets around it). It also maintains dbt sources, runs the DROP release flows,
runs ad-hoc SQL and quality checks. Built into the dev image (`infrastructure/dbt-singlestore-dev-image`) and used in the devcontainer.
Two sibling workspaces (`../release-ui`, `../vaultspeed-framework`) depend on these crates by path and are built by hand. This tree is
a cleaned copy: no docs, fixtures, sample config, golden, differential or live tests (originals are kept locally in `../../local/`).
`README.md` is the manual; load the skill `.agents/skills/vdp-rust-utils` for depth (`.claude/skills/` links to it).

## Where things are

| Path | What |
|---|---|
| `crates/vdp-cli` | Binary `vdp`; `src/bootstrap` is the only composition root; `args.rs`, `help.rs`, `commands/`, `external.rs` (hand-offs), `runtime.rs` |
| `crates/vdp-core` | `Environment` (LDV/ABT/CBT/PRD, aliases dev/val/prd), layered `Config`, `Context`, `ProjectLayout`, `Outcome`, `BatchReport` |
| `crates/vdp-secrets`, `vdp-db`, `vdp-sql-script` | Secret chain and Vault; `SqlSession` over the SingleStore ADBC driver; SQL splitter |
| `crates/vdp-dbt` | dbt engine, grammar (`grammar/dbt.json`), argv scan, guard, `--args`/`--vars` fixes, profile, manifest |
| `crates/vdp-sources`, `vdp-sql`, `vdp-qa` | Sources maintenance; `sql run`/`export`; `qa orphan-tables`/`view-candidates` |
| `crates/vdp-git`, `vdp-jira`, `vdp-release` | Git write guard, Jira, release/patch/promotion flows |
| `README.md`, `.agents/skills/vdp-rust-utils/` | Manual (commands, config, architecture, gaps); skill with `references/` |

## Commands you will run

Preferred, from the repository root: in Docker, so the host needs no Rust toolchain (pinned 1.96, version read from the dev image `Dockerfile`).

```bash
make rust-check W=vdp-rust-utils                 # the whole gate: fmt, clippy (also per feature), tests, docs
make rust-test W=vdp-rust-utils S="-p vdp-dbt"   # only tests (S= goes to cargo test)
make rust-check                                  # all three workspaces: after a change to a public item the siblings use
make rust-cargo W=vdp-rust-utils S="build --release --locked -p vdp-cli"   # any other cargo command
```

With a host `cargo` (rustup installs the pinned toolchain itself) the same checks run from this folder, but `target/` grows large:

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --all-features
cargo clippy -p vdp-cli --all-targets --no-default-features -- -D warnings      # per feature: sources, release, sql, qa
(cd ../release-ui && cargo build --locked --workspace); (cd ../vaultspeed-framework && cargo build --locked --workspace)
```

## Rules that are easy to break

- `dev` is never written: keep `BranchRules` and `GitCli`'s `WriteGuard` in every release code path.
- Argument fidelity of `dbt`: never rewrite argv except the `--args`/`--vars` fixes in `vdp-dbt/src/argv/yaml_arg.rs`. No differential suite here: check by hand.
- Concrete types are named only in `vdp-cli/src/bootstrap`; modules use traits. Every module is a cargo feature of `vdp-cli`.
- A config change touches the settings struct, its `Default`, `MODULE_SECTIONS` and the key tables in `README.md` and `.agents/skills/vdp-rust-utils/references/config.md` (`crates/vdp-cli/tests/config_reference.rs` compares them with the structs).
- Secrets never enter the process environment (`vdp_secrets::child_env`). Help text rules are tested (`args.rs`). Exit codes: 0, 1 (work failed), 2 (could not start).
- `thiserror` in libraries, `anyhow` only in `vdp-cli`, hand-written fakes, no `unsafe`. Public items are shared with the siblings: build both after changing one.
- Never copy hostnames, e-mail addresses or personal paths from the local originals; the cleaned copy uses placeholders (`git.example.com`).
- Do not change the dev image `Dockerfile` casually: Workshop Studio quotes it (`docker exec workshop-studio python -m studio.catalog --check`).
