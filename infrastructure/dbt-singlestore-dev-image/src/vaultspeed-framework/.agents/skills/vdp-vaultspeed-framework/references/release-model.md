# VaultSpeed releases: branch model, guards, flows, hub routes, rights, jobs

Code: `crates/vdp-vaultspeed-release` (the model and the flows) and `crates/vdp-vaultspeed-hub` (the hub extension).
**There is no command line for any of this.** Release management exists only through the hub (`vdp-vaultspeed release
serve`). The crate docs of `vdp-vaultspeed-release` say "used by the CLI and the hub"; the CLI crate uses only
`VsReleaseSettings` (to build the hub's configuration). Settings and defaults: `config.md`.

## Branch model (decisions the flows keep)

- **`main` is the neutral skeleton**: the framework without DAGs or SQL state; deploy state never lands there. A commit on
  `main` may not touch a path under `skeleton_excluded` (staged zips and PML, `deployed/`, `airflow/`, `ledger/`,
  `logs/`); the rewrite rules in `vaultspeed/staging/manual_changes` are skeleton.
- **Environment branches** `LDV`, `ABT`, `CBT`, `PRD` (lowest first) sit on top of `main` and carry their own deploys.
  Nothing flows back to `main` or sideways, and nothing is based on an environment branch: a merge source (a zip branch,
  a release branch, the branch being built) that contains an environment's own commits is **refused**
  (`EnvironmentCommits`: the first-parent history of each environment branch beyond `main`, i.e. merges into it, CI's
  deploy commits, direct fixes).
- **`release/<n>_<TYPE>`** (`zip_prefix`): one branch per VaultSpeed zip, created by VaultSpeed's agent. **Never
  written by the tools** (not in `VsRules::guard()`'s writable list). CI merges each into the auto environment (`LDV`)
  and deploys.
- **`feature/<DROP>`** (`release_prefix`) is a release: built from `main`, collecting zip branches and PML scripts. The
  name must match `^(\d{4}_E?DROP\d{1,2})(\.\d{1,3})?$` (`2026_DROP25`, `2026_DROP25.1`, `2026_EDROP3`); the `.n` numbers
  VaultSpeed releases within one DROP; the Jira fix version is the part before the dot.
- **Merges into environments** use GitLab's merge message `Merge branch '<source>' into '<ENV>'`, because the
  framework's CI deploys on a message that starts with `Merge branch` or `Auto-merge`. Only `direct_merge_environments`
  (`LDV`, `ABT`) take single zip branches; the others take release branches only. A **direct fix** must not start with
  those words (CI would deploy on it).
- A **reset** is a force push of a branch back to an earlier commit of its own, with the dropped commits listed.

## Flows (`vdp-vaultspeed-release/src/flows`)

Each flow takes a `FlowContext` (the `Vcs`, the rules, `expect`, a `RunControl`) and runs as a preview (`dry_run`: same
steps, nothing pushed) or for real. A real run is a **lease on exactly what the preview saw**: `expect` is the origin head
of the target the preview reported, and a moved branch is `VsError::Stale` ("preview again"; through the hub this ends the
job as failed, see Jobs). Pushes use `PushMode::Expect`. Conflicts pause and ask the release manager (see jobs).

| Flow | Does | Refuses / fails when |
|---|---|---|
| `build_release` | `feature/<DROP>` from `main` (or extends the branch on origin; `recreate` starts again from `main`), merges the zip branches in order, commits PML scripts (`PmlChange`: add, replace, or remove by file name), pushes | a zip is not a `release/` branch, listed twice or missing on origin; a zip or the branch holds environment commits; `recreate` drops commits and `accept_losses` is not set; bad PML name (letters, digits, `_ - .`, ends in `.sql`) |
| `merge_into_environment` | merges sources (release branches, or zip branches where allowed) into an environment, last merge's message is what CI sees; reports incoming zips and PML, `LowerWarning`s (zips not yet deployed on the environment below; not blocking), whether the push triggers a deploy | target is not an environment, nothing to merge, environment missing on origin; a source not allowed there or holding environment commits |
| `commit_files` | a direct fix on an environment, a release branch or `main` (skeleton paths only) | other branches; empty message or one that looks like a merge; no files; a skeleton violation on `main` |
| `reset_branch` | back to an earlier commit of the branch (force push) | a branch the guard does not list (zip branches, anything else); target not an earlier commit of the branch; `accept_losses` missing. The guard lists `main`, the environments and `feature/*`, so the guard itself does not stop a reset of `main` (GitLab's force-push right does) |

A merge step ends as `merged`, `resolved` (after a conflict decision), `already` (nothing new), `skipped` (left out at a
conflict) or `planned` (preview). Errors: `VsError::{Git, Refused, Stale, Invalid, Aborted, Zip, Plan}`. Inside a hub job
every one of them ends the job as failed with the message; `hub_error` (`Stale` to 409, `Refused` and `Invalid` to 400, the
rest to 500) serves only the reads (`status`, `changes/file`) and the release-name check of `POST /releases`.

## Status and previews

- `status::status` (from the remote-tracking refs): environments (head, auto/direct flags, zips and PML staged), release
  branches (zips, PML, environments whose branch contains the head), and every zip newest first with its branch, tickets,
  VaultSpeed release, and per environment whether it is staged, **deployed** (the environment's own commit that archived
  it under `archive_dir`, unless a later one removed it; zips `main` carries do not count) and which `deploy_errors_*.csv`
  files its deploy left under `logs_dir`.
- `zips::inspect` reads a zip from git in memory: tickets come from `Comment:` lines of its entries (case-insensitive,
  `ticket_prefix`), the VaultSpeed release from the `DV_NAME` line. Results are cached per blob id. Jira details come
  from the shared tracker; failures are carried as an `error` string per ticket and not cached.
- `preview::plan_preview`: after a merge, the engine planned **offline** on the merged work tree for that environment,
  against the ledger the branch carries (`vaultspeed/ledger/<ENV>.jsonl`, else the legacy `deployed/` state imported in
  memory). No database, no Vault, nothing written. It returns per source how many groups would run or skip, and the
  problems (refusals, halts, errors, grouped with counts), plus the console text `plan` would print.
- `changes`: file and zip-entry diffs between two states (a preview's origin head and its result stay in the object
  database, so they can be compared after a dry run); diffs are limited to 256 KiB.

## The hub extension (`vdp-vaultspeed-hub`)

`VaultspeedExtension` implements `vdp_release_hub::HubExtension` with segment `vaultspeed`; `vdp-vaultspeed-cli/src/hub.rs`
registers it unless `[release_hub.vaultspeed] enabled = false`. `VsHub` owns its **own clone** of the framework repository
(`repo_dir`), its own jobs and a status cache keyed by the refs. After every job the clone returns to a detached HEAD at
`origin/main` without local branches.

**Jobs.** Writes are jobs that run one at a time (a second one is refused as busy) with a step log. A conflict
pauses the job: `GET /jobs/{id}/conflict?path=` returns the three versions of a conflicting file and
`POST /jobs/{id}/resolution` answers with `{ action: continue | skip_step | abort, choices: { <path>: change | branch | delete } }`
(types of `vdp-release`); the wait ends after `[release_hub].conflict_timeout_secs`. A request's `run` object can set
`dry_run`, `on_conflict` (`pause` default, or `theirs`) and `strategy` (`theirs_hunks` default, or `strict`). Result
types in the job's `result`: `vs_release`, `vs_merge` (with the optional offline `plan`), `vs_commit`, `vs_reset`.
What is checked **before** a job starts is an HTTP error: an invalid release name or a branch the guard refuses (400), no
clone yet or another job running (409), a missing right (403). Once the job exists (202) the flow's own failures (a branch
that moved since the preview, a refused source, an aborted conflict, a git error) end it as failed with the message in the
job, not as an HTTP status.

**Rights.** Reading needs a sign-in (when `[release_hub].auth = "gitlab"`). Each write needs the GitLab right on the branch
it changes, and pushes to the framework project as the signed-in person: `push` for releases, merges and commits,
`force` for resets and for `recreate`. A release branch uses the rights of the `feature/*` rule. Clone, re-clone, repair
and resolving a conflict need a push right on `main` or on any environment branch. The rights the UI shows are sampled
for `main`, each environment, `feature/*` and `release/*` and come from GitLab; a GitLab error refuses everything and says
why. The guard also refuses, before any job, a branch that is not `main`, an environment or a release branch. Per route
(`extension.rs`: which arms call `actor_for` or `maintainer`):

| Route | Right needed |
|---|---|
| every `GET` (status, pipeline, file, tree, history, changes, jobs, conflict) | sign-in only |
| `POST /repo/clone`, `/repo/reclone`, `/repo/repair`, `POST /jobs/{id}/resolution` | a push right on `main` or on any environment branch (`maintainer()`) |
| `POST /repo/sync` | none beyond the sign-in: the arm calls no `maintainer()` |
| `POST /releases` | push on the `feature/*` rule; force when `recreate` is set |
| `POST /merges` | push on the target environment |
| `POST /commits` | push on the branch being changed |
| `POST /resets` | force push on the branch being reset |

**Routes** (`vdp-vaultspeed-hub/src/extension.rs`, under `/api/v1/vaultspeed/`; bodies in `api.rs`):

| Route | Does |
|---|---|
| `GET /status[?jira=refresh]` | zips, environments, releases (`refresh` drops cached Jira answers); 409 until the repository is cloned. The hub's own status document carries the extension's summary under `vaultspeed` |
| `POST /repo/clone`, `/repo/reclone` | job: clone, or delete the clone and clone again (body `CloneRequest`; push right needed) |
| `POST /repo/sync` | job: fetch origin and forget cached tickets (no push right needed) |
| `POST /repo/repair` | job: repair the clone (push right needed) |
| `GET /pipeline?sha=` | the GitLab pipeline for a commit (follows CI after a merge); `null` without GitLab |
| `GET /file?branch=&path=` | a file as origin has it (branches shown: `main`, environments, `feature/*`, `release/*`) |
| `GET /tree?branch=&dir=` | the files under a directory |
| `GET /history?branch=&limit=` | recent history of a branch (default 30, 1 to 200), for the reset picker |
| `GET /changes?from=&to=`, `GET /changes/file?from=&to=&path=[&entry=]` | changed files, one file's diff, one zip entry's diff; `from` and `to` are commit ids or `origin/<branch>` |
| `POST /releases` | job: build or extend `feature/<DROP>` (`VsReleaseCreate`: `request`, `expect`, `run`) |
| `POST /merges` | job: merge into an environment (`VsMergeCreate`: `request`, `expect`, `plan`, `run`) |
| `POST /commits` | job: direct fix (`VsCommitCreate`: `request`, `expect`, `dry_run`) |
| `POST /resets` | job: reset a branch (`VsResetCreate`: `request`, `expect`, `dry_run`) |
| `GET /jobs`, `GET /jobs/{id}[?after=]` | the jobs, one job with its steps after a number |
| `GET /jobs/{id}/conflict?path=`, `POST /jobs/{id}/resolution` | the conflict decision |

Status codes (`HubError::status` in `release-ui`): 200 reads, 202 for a started job, 400 refused or invalid before a job
starts (or a bad request body), 401 not signed in, 403 right missing, 404 unknown route or job, 409 no clone yet, a busy hub
or a job not waiting, 500 internal. A flow's failure inside a job is not a status code (see Jobs). Request bodies reject
unknown fields. Tests that pin the contract: `crates/vdp-vaultspeed-hub/tests/hub.rs`.

## TypeScript bindings

Behind the `ts` feature of `vdp-vaultspeed-release` and `vdp-vaultspeed-hub`; `ts.rs` lists every type. The hub's own
generator (`vdp_release_hub::typescript_with`) appends them to its types so the frontend has one `types.ts`:

```
VDP_TS_OUT=<file> cargo test -p vdp-vaultspeed-hub --features ts export_bindings
```

The test asserts `VsSummary` and the hub's `Analysis` are present, and that no `bigint` appears. A new request or
response type needs its `#[cfg_attr(feature = "ts", derive(ts_rs::TS))]` and a line in `declarations()`.
