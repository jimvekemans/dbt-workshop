# release-ui

The backend of the **release UI**, as a Rust workspace next to `vdp-rust-utils` and `vaultspeed-framework`. The release
UI is the web application release managers use to cut, patch and promote dbt DROP releases: the board of tickets, the
conflicts to decide, the announcement mail. The Svelte frontend is a separate repository; this workspace is the server
behind it.

One binary, `vdp-release-ui`: the whole `vdp` command line (dbt wrapper, sources, releases, ...) plus **`release serve`**,
which starts the release hub next to `vdp`'s own release commands (`release create|analyse|patch|promote|branches`).
`vdp release-ui ...` runs this binary (see `vdp-rust-utils/README.md`).

```
vdp-release-ui release serve                                          # the hub, with the `[release_hub]` settings
vdp-release-ui release serve --repo-dir /opt/project --port 8765 --ui-dir ../release-ui-frontend/build
vdp-release-ui release serve --cors-origin http://localhost:5173      # a frontend on `npm run dev`
vdp-release-ui release serve --validate                               # run `[release] validate` (dbt) before a push
vdp-release-ui release analyse --fixversion 2026_DROP05 --json        # every other `vdp` command works as well
vdp release-ui release serve                                          # the same, through the `vdp` binary
```

`release serve` prints `release hub on http://<address>/api/v1 (repository <dir>[, UI from <dir>])` on stderr once it
listens (also under `-q`), and runs until it is stopped. `--port 0` picks a free port.

## What the hub is

A server around the release flows of `vdp-release` (the same code `vdp release ...` runs): one clone of the dbt
repository (`[release_hub].repo_dir`, `[release_hub].repo_url`), analyses stored with their selection, git work as jobs run
one at a time with a step log, sign-in with a personal GitLab token (`[release_hub].auth = "gitlab"`, or `"none"` for local
use), and the announcement of a release by mail (`[notify]`, sent through SMTP). Credentials never reach the browser: Jira
and SMTP come from the vdp secret chain. The API is `/api/v1`. Further repositories plug in as **extensions** below
their own URL segment: `vdp-vaultspeed` (the `vaultspeed-framework` workspace) adds the VaultSpeed releases at
`/api/v1/vaultspeed/...` through `vdp_release_ui_cli::hub::serve` and its own extension factory.

### Serving the frontend

The Svelte frontend is built in its own repository. `--ui-dir <dir>` (else `[release_hub].ui_dir`) names its build output:
the hub then serves those files next to the API, and answers unknown paths with `index.html` (a single-page app). The
directory must hold an `index.html`, or the command refuses to start. Without `--ui-dir` the hub is API only (in the
workshop's deployment nginx serves the frontend). While developing the frontend, run it with `npm run dev` and start the
hub with `--cors-origin http://localhost:5173`.

## Build

```
cargo build --release --locked -p vdp-release-ui-cli      # target/release/vdp-release-ui
```

The workspace imports the `vdp-*` toolkit crates by path (`../vdp-rust-utils/crates/<name>`), so both folders must sit
next to each other. The external dependencies have the versions of `vdp-rust-utils`, and `Cargo.lock` was started from its
lock file (cargo pruned what this workspace does not use). Rust 1.96 (`rust-toolchain.toml`). Put `vdp-release-ui` next to
`vdp` or on `PATH`, so that `vdp release-ui ...` finds it. The dev image builds `vdp` only; build this workspace by hand.

## Crates

| Crate | Content |
|---|---|
| `vdp-release-hub` | the hub: clone, analyses, jobs, sign-in, the HTTP API and static frontend, the extension interface (`HubExtension`) |
| `vdp-notify` | the release announcement (HTML and text) and sending it by SMTP |
| `vdp-release-ui-cli` | the composition root and the `vdp-release-ui` binary; as a library, `args` (the command line), `hub::serve` and the `hub::ExtensionFactory` a binary with its own extensions passes |

Dependencies between the three workspaces:

```
vdp-rust-utils  <--  release-ui  <--  vaultspeed-framework
(vdp, vdp-release,    (hub, notify,     (VaultSpeed, its hub extension,
 vdp-cli, ...)         vdp-release-ui)   vdp-vaultspeed)
```

`vdp-release-ui-cli` embeds `vdp-cli` as a library: it parses `vdp_cli::args::GlobalArgs` and its own command enum (`release
serve`, or any `vdp` command), runs every other command through `vdp_cli::commands::run_command`, and runs the dbt
validation of `--validate` through `vdp_cli::commands::run_dbt_validation`. This workspace never depends on
`vaultspeed-framework`.

## Configuration

The same `vdp.toml` as `vdp`, layered the same way. The hub reads `[release_hub]` (clone, address, frontend, jobs, sign-in,
`[release_hub.<extension>]` tables), `[notify]` (SMTP, default recipients), and the sections of the release flows
(`[release]`, `[jira]`, `[git]`). `vdp` itself knows `[release_hub]` and `[notify]` as module sections, so it does not
warn about them.

## TypeScript bindings

The frontend's API types are generated from the Rust types with ts-rs, behind the `ts` feature (chained through
`vdp-release/ts` and `vdp-notify/ts`):

```
VDP_TS_OUT=/tmp/api.ts cargo test -p vdp-release-hub --features ts export_bindings
```

writes the whole module of API types to `$VDP_TS_OUT`. The VaultSpeed extension contributes its own types in
`vaultspeed-framework` (`cargo test -p vdp-vaultspeed-hub --features ts export_bindings`), appended to the hub's through
`vdp_release_hub::typescript_with`.

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features`, `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`; also with
  `cargo +stable`.
- `vdp-release-hub`: unit and doc tests; `tests/extension.rs` (a fake extension: routing below its segment, the status
  keyed by segment, rights and job results under its own names) and `tests/clone_lifecycle.rs` (clone, identity, protected
  branch, repair, sync and re-clone of the hub's clone against a local bare repository).
- `vdp-notify`: the announcement's HTML and text, SMTP against a stub.
- `vdp-release-ui-cli` (`tests/process.rs`): `release serve` prints its listening address as a process even with `-q`;
  `release serve --help` and `release analyse --help` (a command of `vdp`) work; an unknown command is clap's error.
- The VaultSpeed side of the hub is tested in `vaultspeed-framework` (`vdp-vaultspeed-hub`), which builds on this workspace.

Not exercised here: GitLab sign-in against a real GitLab, Jira, SMTP against a real server, and pushes to the remote.
