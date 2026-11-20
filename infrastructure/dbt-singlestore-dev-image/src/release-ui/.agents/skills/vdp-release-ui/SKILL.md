---
name: vdp-release-ui
description: >-
  The release-ui Rust workspace (crates vdp-release-hub, vdp-notify, vdp-release-ui-cli; binary `vdp-release-ui`, also
  reached as `vdp release-ui`): `release serve`, the release hub, the HTTP API `/api/v1` behind the web application
  release managers use to analyse, cut, patch and promote dbt DROP releases, with GitLab token sign-in, one cloned
  repository, git jobs one at a time with conflict pauses, stored analyses, release announcements by SMTP, the hub
  extension interface (`HubExtension`) and the generated TypeScript bindings (ts-rs, `typescript_with`). Use it when
  touching crates/vdp-release-hub, vdp-notify or vdp-release-ui-cli, a `[release_hub]` or `[notify]` setting, a route,
  error or job of the hub API, the sign-in or rights rules, an extension (vaultspeed), or when asked how this
  standalone workspace fits the workshop stack, is built, tested or would be deployed.
---

# release-ui (`infrastructure/dbt-singlestore-dev-image/src/release-ui`)

Read `README.md` for the user-facing reference and `CLAUDE.md` for orientation; this file is the working guide. Depth
is in `references/`.

## When to use

Any change to the hub, its HTTP API, settings, sign-in, jobs, announcements or the `vdp-release-ui` command line; any
question about what the hub does. Not for the release *flows* (analysis, revert/patch/promote logic, git and Jira
adapters): those are `vdp-release`, `vdp-git`, `vdp-jira` in `vdp-rust-utils`
([skill](../../../../vdp-rust-utils/.agents/skills/vdp-rust-utils/SKILL.md)). Not for the workshop's own release flow
(tags, Airflow `dbt_release`): that is `.agents/skills/dbt-release-flow/SKILL.md` at the repository root.

## Map

| Crate | Responsibility | Entry file |
|---|---|---|
| `vdp-release-hub` | the hub: clone, analyses, jobs, GitLab sign-in and rights, HTTP API, static frontend, extension interface, TypeScript export | `src/http.rs` (`route`), `src/hub/mod.rs` (`Hub`), `src/lib.rs` |
| `vdp-notify` | the announcement (subject, HTML, text) and delivery (`Mailer`, `SmtpMailer`, `Delivery::PreviewOnly`) | `src/lib.rs`, `src/mail.rs` |
| `vdp-release-ui-cli` | composition root, the `vdp-release-ui` binary: `args::{Cli, Command, ServeArgs}`, `hub::{serve, ExtensionFactory, ExtensionInputs, no_extensions}`, `run_command` | `src/main.rs`, `src/hub.rs`, `src/args.rs` |

Hub modules: `api` (bodies), `auth` (GitLab, `Sessions`), `clone_manager` (clone, sync, repair, health; also used by
extensions), `error` (`HubError`), `extension`, `http`, `hub/{analyses, releases, announcements, host}`, `jobs`,
`settings`, `ts`. Stack: `tiny_http` (thread per request, bodies capped at 1 MiB), `ureq` (GitLab), `lettre` (SMTP),
`ts-rs` (feature `ts`); no async runtime.

## How it fits the stack

- **Standalone, not in the workshop stack.** No compose service, stack Makefile target (only the Rust checks, `make rust-check W=release-ui`, see
  Verify), nginx route, overview entry, CI job or manager step; no image contains it; `dbt-singlestore-workshop/vdp.toml` has no `[release_hub]` or `[notify]`.
  nginx here serves only the dbt docs sites (8081, 8082, 8086). It is unrelated to the Airflow `dbt_release` DAG: the
  hub drives git, Jira and GitLab for the client's DROP releases (`release/<DROP>`, `INTEL-n`, `changelog/<DROP>.md`),
  not Airflow or SingleStore. The Svelte frontend is a separate repository.
- **Build and run by hand:** `cargo build --release --locked -p vdp-release-ui-cli` in this folder (Rust 1.96 from
  `rust-toolchain.toml`) gives `target/release/vdp-release-ui`. The dev image builds only `-p vdp-cli` from
  `vdp-rust-utils`. The workshop wiring is a plan: `references/deployment.md`.
- **Dependencies:** `vdp-rust-utils <- release-ui <- vaultspeed-framework`, all by path
  (`../vdp-rust-utils/crates/<name>`), so the three folders must sit side by side in `src/`. This workspace never
  depends on `vaultspeed-framework`; `vdp-vaultspeed-hub` consumes this crate's public API (`CloneManager`, `Actor`,
  `GitLab`, `HubError`, `http::{API, Reply, route}`, `jobs`, `typescript_with`, ...) and `vdp-vaultspeed-cli` runs
  `vdp_release_ui_cli::run_command` with its own extension factory
  ([skill](../../../../vaultspeed-framework/.agents/skills/vdp-vaultspeed-framework/SKILL.md)).
- **Hand-off from `vdp`:** `vdp release-ui <args>` (HANDOFFS in `vdp-rust-utils/crates/vdp-cli/src/external.rs`) execs
  `vdp-release-ui <args>` with the word `release-ui` removed, found next to `vdp` or on `PATH`; exit 2 with a message
  if missing. This binary rejects `vdp`'s own hand-offs (`vaultspeed ...`) as unrecognised subcommands.
- **Config:** the same `vdp.toml` layering as `vdp`; `release serve` reads `[release_hub]`, `[notify]`, `[release]`,
  `[jira]` (`references/config.md`).

## Rules that are easy to break

1. **Credentials stay on the server.** Jira and SMTP secrets come from the secret chain and never reach a response;
   the API reports readiness only. The one exception is the GitLab token a person POSTs to `/session`: held in memory
   for 12 hours, used for that person's pushes and rights, never logged (`Debug` impls redact; `HttpRequest`'s `Debug`
   prints query keys only). Do not log bodies, cookies or tokens; do not put credentials in `repo_url` (`GET /status`
   shows the origin URL).
2. **`dev` is never written**, not even locally. `BranchRules` validates targets (`release_target`) and `GitCli`'s
   `WriteGuard` (from `rules.guard()`) allows only `release/*` and `main`; writes go through `hub.git_as(&actor)`,
   which keeps the guard. The clone keeps no local branches (`detach`, `tidy`). `main` changes only by promotion.
   Never add a path that checks out or pushes `dev`.
3. **One git job at a time** on one clone (`Jobs::start` returns `Busy`). Anything that changes the work tree is a
   job, not work done inside a request thread.
4. **Execute is bound to the preview**: stale analyses are refused, `expect`/`expect_main` and the `accept_*` flags
   are required, only the newest two release branches can change, and the hub has no `--allow-old-release`.
5. **Writes need release rights** (`writer()` in `route`): clone, repair, re-clone, releases, patches, promotions,
   resolutions. With `auth = "none"` nothing is checked; keep that for local use.
6. **Every leaf command needs `about`, `long_about` and an `Examples:` block** in `after_long_help`, and every option
   needs help; lines stay within 100 columns (`long_about` 96, option `long_help` 86). The tests
   `every_command_and_option_has_help_text` and `help_lines_stay_within_100_columns` enforce it.
7. **An API change updates `ts.rs` and the generated TypeScript** (and `references/api.md`, the README route table,
   the frontend). A new `i64`/`u64` field needs `ts(type = "number")` (the export test refuses `bigint`).
8. **An extension lives below its own segment** and reaches the hub only through `HubExtension`, `ExtensionRequest`
   and `ExtensionHost`; the core knows nothing of what it does. A segment equal to a core first segment (`status`,
   `session`, `identity`, `repo`, `jobs`, ...) shadows that core route, because the extension is matched first. Do not
   name one `dbt`: that prefix is dropped before the lookup, so it would be reachable only as `/api/v1/dbt/dbt/...`.
9. **This workspace never depends on `vaultspeed-framework`**, and a change in `vdp-rust-utils` (`vdp-release`,
   `vdp-git`, `vdp-jira`, `vdp-cli`, `vdp-core`, `vdp-secrets`) can break this workspace: build and test it, and
   `vaultspeed-framework`, after such a change.
10. **Do not change decisions silently** (README "Decisions the code encodes"): conflicts pause for the release
    manager, force-push rights on `main` and `release/*` decide who releases, every release ends with
    `changelog/<DROP>.md`, release-only fixes reach `dev` by the developers' own merge request.

## Playbooks

**Add a route.**

1. `http.rs`: a match arm in `route` before the `_` arm (mind arm order for paths with slashes); status 202 for a job,
   `writer()` for writes.
2. `hub/*.rs`: a `Hub` method returning `Result<_, HubError>`, with an errors section in its doc (clippy warns
   otherwise); a job via `self.jobs.start(JobKind::..., title, |log| ...)` returning a `JobResult`.
3. Types: request and response types derive `Serialize`/`Deserialize` and, under
   `#[cfg_attr(feature = "ts", derive(ts_rs::TS))]`, `TS`. They live in `api.rs` (bodies, status, analyses), `jobs.rs`
   (`Job`, `JobKind`, `JobStatus`, `JobStep`, `Level`, `JobResult`) and `auth.rs` (`SessionUser`, `BranchRight`,
   `Pipeline`); the flows' types come from `vdp-release` and `vdp-notify`. A request type gets
   `#[serde(deny_unknown_fields)]` (as all but `GitIdentity` and `Resolution` have), optional fields
   `#[serde(default)]` plus `#[cfg_attr(feature = "ts", ts(optional))]`.
4. `ts.rs`: add the new types to `typescript_with`.
5. A test that calls `route(&hub, HttpRequest { .. })` in `crates/vdp-release-hub/tests/` (`extension.rs` shows how to
   build a `Hub`).
6. The README route table and `references/api.md`. The README's counts (tests, TypeScript declarations; "Testing")
   change when a route or a test is added: update them there, and only there.

**Add a hub setting.** Field plus doc comment in `HubSettings` (`settings.rs`) and its `Default`; wire it into
`HubConfig` in `hub::serve` (`vdp-release-ui-cli/src/hub.rs`); for an option add it to `ServeArgs` (`args.rs`) with a
`long_help` constant in `help.rs`; update `references/config.md` and the README table. `[notify]` keys go to
`NotifySettings` (strict: `deny_unknown_fields`).

**Add an extension.** Follow README "Extensions": implement `HubExtension` in a crate that depends on
`vdp-release-hub` only; read `[release_hub.<segment>]` with `HubSettings::extension::<T>`; build it in an
`ExtensionFactory` and run the command line with `vdp_release_ui_cli::run_command(&global, command, &factory)`; export
TypeScript with `declaration::<T>()` and `typescript_with`. Model: `vdp-vaultspeed-hub/src/extension.rs` and
`vdp-vaultspeed-cli/src/hub.rs` in `vaultspeed-framework`; test it like `tests/extension.rs`.

**Add a command.** A command of `vdp` belongs in `vdp-rust-utils` and appears here through `CoreCommand` by itself. A
command that only this binary has: add a `Command` variant in `args.rs` (the hand-written `FromArgMatches` and
`Subcommand` impls: `release serve` is hung below `release` in `augment_subcommands`), run it in `run_command`, write
its help in `help.rs`, and extend `ROOT_AFTER_LONG_HELP` ("What this binary adds"). The help tests of
`vdp-release-ui-cli` (`cargo test -p vdp-release-ui-cli`) tell what is missing.

**Regenerate the TypeScript bindings.**

```
VDP_TS_OUT=<file> cargo test -p vdp-release-hub --features ts export_bindings
```

writes the hub's module (declaration count: README "Testing"). The frontend keeps it at
`frontend/src/lib/api/types.ts` in its own repository (per the doc comment in `ts.rs`). When the VaultSpeed extension is
in use the frontend takes the complete file from `vaultspeed-framework` instead: `cargo test -p vdp-vaultspeed-hub
--features ts export_bindings` there writes the hub's types plus VaultSpeed's (it calls `typescript_with`), so
regenerate that file after an API change.

**When `vdp-release` (or `vdp-git`, `vdp-jira`, `vdp-cli`) changes.** This workspace calls `Analyser`, `ReleaseFlow`,
`PatchFlow`, `Promotion`, `BranchRules`, `check_scope`, `detect`, `release_branches`, `changes::*` and
`Resolution`/`ConflictReport`, and exposes their serde types in the API and the TypeScript. After a change there:
build this workspace and `vaultspeed-framework`; if a serialised type changed (field, rename, enum value) it is an API
change (previous playbook, `references/api.md`); if the `vdp` command tree changed, `args.rs` tests (`serve` only
below `release`, help rules) may fail first.

## Verify

Prefer the Docker targets of the root `Makefile`: the host needs no Rust toolchain, and the registry and build output live in Docker
volumes, not in `target/`. From the repository root:

```
make rust-check W=release-ui                   # fmt, clippy (all features), tests (default features), docs
make rust-test W=release-ui S="-p vdp-notify"  # a quick loop: only tests (S= goes to cargo test)
make rust-check                                # after an API change: this workspace and the two it touches (vdp-rust-utils, vaultspeed-framework)
```

`make rust-cargo W=release-ui S="<cargo arguments>"` runs any other cargo command in the same container (`make rust-clean` removes its volumes).
The TypeScript export writes a file you choose (`VDP_TS_OUT`): run that one with a host `cargo`.

With a host `cargo`, run in this folder on the pinned toolchain (all pass at the time of writing; counts: README "Testing"):

```
cargo fmt --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --locked
```

The rule from the old README is to run the same with `cargo +stable` too (not run in this round). After an API change
also build and test `vaultspeed-framework`, which depends on this workspace:

```
cd ../vaultspeed-framework && cargo test --workspace --locked
```

The first build there is slow because of DuckDB (bundled): about 80 s cold with `make rust-check W=vaultspeed-framework`. Use `CARGO_BUILD_JOBS=4` on
a busy host. Hand check: start `release serve` on a throwaway local bare
repository with `auth = "none"` and `curl` the routes (README "Try it without GitLab or Jira").

## Known gaps

No TLS, no signal handling, one clone and one job for everyone, public `GET /status`, no frontend here, and the tests do
not cover GitLab sign-in over HTTP beyond `GitLab::rights`, the release/patch/promotion jobs, the conflict pause end to
end or SMTP delivery (README "Testing", "Known gaps").

## References

- [references/api.md](references/api.md): read when adding or changing a route, a request or response type, an error,
  the session or rights rules, or when a client gets an unexpected status.
- [references/config.md](references/config.md): read when adding or changing a `[release_hub]`, `[notify]`,
  `[release]` or `[jira]` key, a secret, or when the hub will not start.
- [references/deployment.md](references/deployment.md): read when someone decides to run the hub in the workshop stack
  (plan, not implemented).
