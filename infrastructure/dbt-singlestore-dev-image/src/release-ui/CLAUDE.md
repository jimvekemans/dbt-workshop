# release-ui: context for Claude

The backend of the release UI: a Cargo workspace (Rust 1.96, edition 2024) with the release hub, the announcement mailer and the
`vdp-release-ui` binary (the whole `vdp` command line plus `release serve`). A cleaned copy of client code, kept next to
`vdp-rust-utils` (which it depends on by path) and `vaultspeed-framework` (which depends on it).

**It is not part of the workshop stack.** No compose service, stack Makefile target (`up`, `bootstrap`, `test`, ...), nginx route, overview entry, CI job or manager
step uses it, and no image contains it; the root Makefile only checks its code (`make rust-check W=release-ui`, below). It is unrelated to the workshop's Airflow `dbt_release` DAG and tag-based release flow (root `.agents/skills/dbt-release-flow/`).
The Svelte frontend is a separate repository. Facts come from the code; `README.md` is the reference, the skill is the guide:
[`.agents/skills/vdp-release-ui/SKILL.md`](.agents/skills/vdp-release-ui/SKILL.md) (read it before changing anything; `.claude/skills/vdp-release-ui` is a link to it).

## Where things are

| What | Where |
|---|---|
| HTTP routes, static frontend, CORS | `crates/vdp-release-hub/src/http.rs` (`route`, `HubServer`) |
| The hub: analyses, releases, announcements, status | `crates/vdp-release-hub/src/hub/` |
| Jobs (one at a time, step log, conflict pause) | `crates/vdp-release-hub/src/jobs.rs` |
| GitLab sign-in, rights, sessions | `crates/vdp-release-hub/src/auth.rs`, `hub/host.rs` |
| Request and response bodies, errors, TypeScript | `src/api.rs`, `src/error.rs`, `src/ts.rs` |
| Extension interface (`HubExtension`) | `crates/vdp-release-hub/src/extension.rs` |
| Settings | `vdp-release-hub/src/settings.rs`, `vdp-notify/src/settings.rs`; the rest in `vdp-rust-utils` |
| Announcement and SMTP | `crates/vdp-notify/src/` |
| Command line and `release serve` start-up | `crates/vdp-release-ui-cli/src/{args,help,hub,main}.rs` |
| Release flows, git, Jira (not here) | `../vdp-rust-utils/crates/{vdp-release,vdp-git,vdp-jira}` |

## Commands

Preferred, from the repository root: in Docker, so the host needs no Rust toolchain (pinned 1.96, version read from the dev image `Dockerfile`; no Python).

```
make rust-check W=release-ui                    # the whole gate: fmt, clippy (all features), tests, docs
make rust-test W=release-ui S="-p vdp-notify"   # only tests (S= goes to cargo test)
make rust-cargo W=release-ui S="build --release --locked -p vdp-release-ui-cli"   # any other cargo command
```

The registry and the build output live in Docker volumes (`make rust-clean` removes them), not in `target/`. With a host `cargo` (rustup installs the pinned
toolchain itself) the same checks run in this folder; `target/` (gitignored) grows large:

```
cargo build --release --locked -p vdp-release-ui-cli                         # target/release/vdp-release-ui
cargo fmt --check && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked                               # test counts: README, "Testing"
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --locked
VDP_TS_OUT=/tmp/api.ts cargo test -p vdp-release-hub --features ts export_bindings   # writes a file: host cargo
```

`make rust-check` runs the tests with the default features (38 tests); `--all-features` on the host adds the `ts` export test.
The dev image, compose file and the stack's Makefile targets do not know this workspace (a plan for wiring it in: `.agents/skills/vdp-release-ui/references/deployment.md`).

## Rules that are easy to break

- Credentials stay on the server; only a person's GitLab token is POSTed to `/session` (12 h in memory). Never log tokens, cookies or bodies.
- `dev` is never written (`BranchRules` plus `GitCli`'s `WriteGuard`); only `release/*` and `main` (by promotion). One git job at a time.
- Every leaf command needs `about`, `long_about`, `Examples:` and option help, within 100 columns (tests enforce it).
- An API change updates `ts.rs` and the TypeScript, `.agents/skills/vdp-release-ui/references/api.md` and the README route table.
- Extensions live below their own segment; this workspace never depends on `vaultspeed-framework`.
- A change in `vdp-rust-utils` can break this workspace: build and test it, and `vaultspeed-framework`, afterwards.
- No organisation hostnames, e-mail addresses or personal paths in files here (placeholders: `git.example.com`, `person1@example.com`);
  the one exception is the licence identifier in `Cargo.toml` (`LicenseRef-VDAB-internal`), which stays as the original licence statement.
