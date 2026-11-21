# Image, consumers and the two sibling workspaces

Read when you change how `vdp` is built or installed, touch the dev image's Dockerfile, change something the workshop relies on
(`vdp --version`, the `dbt` link, `vdp.toml` keys, `vdp:` tasks), bump the Rust toolchain, or change a public item that the sibling
workspaces use. Paths are relative to the repository root unless they start with `src/` (then relative to
`infrastructure/dbt-singlestore-dev-image/`, the "dev image folder"). Everything below was read from the files named.

## How vdp gets into the dev image

`infrastructure/dbt-singlestore-dev-image/Dockerfile`:

| Step | What it does |
|---|---|
| `ARG BASE_IMAGE=dbt-singlestore-base:1.11.11`, `ARG RUST_VERSION=1.96` | Base image and the Rust image tag of the build stage. |
| Stage `vdp-build` (`FROM rust:${RUST_VERSION}-trixie`) | Debian trixie, the same glibc as the base image, so the binary always runs there. |
| `ARG CARGO_BUILD_JOBS=` | Passed to cargo only when non-empty; `docker build --build-arg CARGO_BUILD_JOBS=2` on a busy host (the release compile is memory-hungry). |
| `COPY Dockerfile vdp* /prebuilt/` | A prebuilt `./vdp` (gitignored) lands in `/prebuilt/vdp`; the Dockerfile is listed so the `COPY` never matches nothing. |
| `RUN --mount=type=bind,source=src,target=/src-all` plus cache mounts for the cargo registry, git and `/cargo-target-vdp` | The whole `src/` folder is mounted, not `src/vdp-rust-utils`, because a mount of a missing folder fails the build. |
| Inside the `RUN` | `/prebuilt/vdp` exists: copy it, done. Else no `/src-all/vdp-rust-utils/Cargo.toml`: print a message, build the image without vdp. Else `cd /src-all/vdp-rust-utils && CARGO_TARGET_DIR=/cargo-target-vdp cargo build --release --locked -p vdp-cli` and copy `release/vdp` to `/out/vdp`. |
| Final stage `FROM ${BASE_IMAGE}` | `COPY --from=vdp-build /out/ /tmp/vdp-out/`, then `install -m 755 vdp /usr/local/bin/vdp`. If `/usr/local/bin/dbt` is not yet a symlink, the engine moves to `/usr/local/libexec/dbt/dbt` and `/usr/local/bin/dbt` becomes a link to `vdp`. |
| `ENV VDP_DBT_BIN=/usr/local/libexec/dbt/dbt DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true DBT_SEND_ANONYMOUS_USAGE_STATS=false DO_NOT_TRACK=1` | How `vdp` finds the engine; dbt settings. |
| `USER dbt:root`, `WORKDIR /usr/app/dbt/workspace`, `CMD ["dbt", "--version"]` | The `dbt` user of the base image (home `/usr/app/dbt`). |

- Only default features are built (`-p vdp-cli`): `sources`, `release`, `sql`, `qa`. The image installs only `vdp`, so `vdp vaultspeed`
  and `vdp release-ui` hand-offs find no binary and exit 2 with an explanation.
- The ADBC driver stays at `/usr/local/lib/dbt/libadbc_driver_singlestore.so`. `vdp` finds it like dbt does: `DriverSource::locate` walks
  up from the engine (`/usr/local/libexec/dbt/dbt`) to `/usr/local`, then looks in `lib/dbt/`. Keep the engine and the driver in those
  two layouts or set `$VDP_ADBC_DRIVER`.
- Without vdp (no prebuilt file and no source) `dbt` stays the plain engine; sqlfmt, dbt-bouncer and pre-commit are unaffected.
- `.dockerignore` of the dev image folder excludes `**/target`, `**/.git`, `**/.venv`, `**/__pycache__`, `**/node_modules`,
  `src/*/.claude`, `src/*/.agents` and `local`. The skills (`.agents`) and their `.claude` links are therefore not in the build
  context; the Markdown files are, and the bind mount covers all of `src/`, so an edit under it, a README included, changes the input
  of that `RUN` (the cargo cache mounts are meant to keep that rebuild incremental; no build-time measurement is recorded).
- Build: `docker build -t dbt-singlestore-dev:1.11.11 infrastructure/dbt-singlestore-dev-image` (the root `make images` runs it after
  the base image). Check an image: `docker run --rm dbt-singlestore-dev:1.11.11 vdp --version`, and
  `docker run --rm -e VDP_NO_VAULT=1 dbt-singlestore-dev:1.11.11 vdp env doctor`.
- The grammar cache is written to `$XDG_CACHE_HOME/vdp`, else `$HOME/.cache/vdp` (the `dbt` user's home is `/usr/app/dbt`). If that
  directory is not writable the grammar is derived again on every `dbt` call (one `dbt <command> --help` process per command); the
  embedded grammar (`[dbt] grammar = "embedded"`) avoids that.

## Consumers

| Consumer | File | What it relies on |
|---|---|---|
| Devcontainer | `dbt-singlestore-workshop/.devcontainer/devcontainer.json` | Image `dbt-singlestore-dev:1.11.11` on network `workshop_net`; user `dbt`. |
| Dotenv | `dbt-singlestore-workshop/.devcontainer/post_create.sh` | Writes `.devcontainer/.env` (`SINGLESTORE_HOST/USER/PASS/DB`, `DBT_DEPLOY_TARGET=dev`; when the developer chooses the vault way, `SINGLESTORE_PASS` is replaced by the two AppRole ids `KEYVAULT_ROLE_ID` and `KEYVAULT_SECRET_ID`): the file `[core].dotenv` points at. It feeds secrets and the child environment of dbt; the environment of `vdp` itself is selected from the process variable or `default_environment`, not from the dotenv file. It also runs `dbt deps` and `pre-commit install`. |
| Configuration | `dbt-singlestore-workshop/vdp.toml` | `[core] default_environment = "dev"`, `dotenv = ".devcontainer/.env"`; `[environments.dev\|val\|prd]` with `vault_url = "http://vault:8200"`, an empty `ca_certificate` and `role_id_vars` / `secret_id_vars` (aliases of `LDV`, `ABT`, `PRD`); `[secrets] vault_mount = "kv-workshop"`, `precedence = ["vault:vdp/{env_short}/dbt", "dotenv", "env"]`; `[sources] databases` and `[sources.database_prefix]`; `[qa] run_results_table = "{env_short}_vdp_mgmt.dbt_run_results"`. A renamed or retyped key here is an error for every command. |
| VS Code tasks | `dbt-singlestore-workshop/.vscode/tasks.json` | The `vdp:` tasks run `vdp env show`, `vdp env doctor`, `vdp sources add [--dry-run] <tables>`, `vdp sources add --missing`, `vdp sources rewrite`, `vdp sql run <file>`, `vdp sql export <table>`, `vdp qa orphan-tables`, `vdp qa view-candidates`, with `DBT_DEPLOY_TARGET=dev`. Flag renames break them. |
| Pre-commit | `dbt-singlestore-workshop/.pre-commit-config.yaml` | The hook `dbt parse` calls `dbt` (the wrapper); `parse` is not guarded. |
| Test setup | `dbt-singlestore-workshop/tests/setup_tests.sh` | Calls `dbt compile`, `dbt build -s state:modified ...` (selected, so the guard passes). |
| Workshop Studio, probe | `infrastructure/workshop-studio/backend/studio/actions/images.py` | `PROBES["helper_tool"]` runs `vdp --version` in a throwaway dev-image container and reads `(\d+\.\d+\.\d+\S*)` from the first output line. `vdp --version` must keep printing `vdp <major>.<minor>.<patch>`; the version comes from `[workspace.package] version`. |
| Workshop Studio, dbt runs | `infrastructure/workshop-studio/backend/studio/actions/dbt.py` | Runs `dbt ...` as developer users in the dev image, `DEV_IMAGE = "dbt-singlestore-dev:1.11.11"` (the tag is hard-coded), so the wrapper's guard applies: unselected `build`/`run` is refused for personal `pu*` users, allowed for `*_dbt` service users. |
| Workshop Studio, toolbox | `infrastructure/workshop-studio/backend/studio/actions/toolbox.py` | `toolbox.run` runs the shell commands of a scenario (`dbt ...`, `vdp ...`) in a throwaway container of the same `DEV_IMAGE`, with the project copied out of the deployed package, as a developer (`auth: password`, `file`, or `vault`: a `.devcontainer/.env` with only `KEYVAULT_ROLE_ID` and `KEYVAULT_SECRET_ID`, so the Vault login of `vdp.toml` is exercised) or as the robot. `expect: refused` is true only when the exit code is not 0 and an output line starts with `vdp:`: keep that prefix on the guard's refusals. Scenario: `developers-toolbox.yaml`. |
| Workshop Studio, scenario | `infrastructure/workshop-studio/scenarios/same-toolbox.yaml` | Two quotes (`max_lines: 12` each). First `.devcontainer/devcontainer.json` of the workshop project, from the literal text `"image": "dbt-singlestore-dev:1.11.11",` to `"workspaceFolder"`, with notes anchored on `"image"`, `"runArgs"` and `"workspaceFolder"`. Second `dev-image/Dockerfile`, from the line starting `RUN --mount=type=bind,source=src/sqlfmt` to the text `ln -s`, with notes anchored on `python3 -m venv`, `pip install` and `ln -s`. |
| Studio mount | `infrastructure/compose/docker-compose.yml` | Mounts `../dbt-singlestore-dev-image` read-only at `/workshop/dev-image` in the studio. |

After touching the Dockerfile or `devcontainer.json`, check the scenario anchors with
`docker exec workshop-studio python -m studio.catalog --check` (it needs the stack running). Anchors that must stay as written:
- Dockerfile: the line starting `RUN --mount=type=bind,source=src/sqlfmt`, the text `python3 -m venv`, `pip install` and `ln -s` after it,
  all within 12 lines; insert new stages before or after that block, not inside it.
- `devcontainer.json`: the literal `"image": "dbt-singlestore-dev:1.11.11",` first, `"runArgs"` after it and `"workspaceFolder"` closing the
  quote, all within 12 lines. A new image tag or a reordered file breaks the check.
- The tag `dbt-singlestore-dev:1.11.11` is also hard-coded in `DEV_IMAGE` (`studio/actions/dbt.py`), the `from:` line of the scenario, the
  root `Makefile` (`images`), `infrastructure/manager/bootstrap_stack.py` (`DEV_IMAGE_NAME`) and as recorded text in
  `workshop-studio/recordings/same-toolbox.json`: bump them together.

Not consumers: `dbt-singlestore-workshop/.gitlab-ci.yml`, the project `Dockerfile` (image `dbt-vdp:<tag>`) and the Airflow DAGs run the
plain `dbt` of the base image, never `vdp`. `dbt-vdp` is the project image's name, unrelated to this toolkit.

## The sibling workspaces

`src/release-ui` (binary `vdp-release-ui`, crate `vdp-release-ui-cli`) and `src/vaultspeed-framework` (binary `vdp-vaultspeed`,
crate `vdp-vaultspeed-cli`) are Cargo workspaces of their own next to this one. They are not built into the image and nothing in the
stack references them. Their dependencies run `vdp-rust-utils` <- `release-ui` <- `vaultspeed-framework`, by path:
`../vdp-rust-utils/crates/<name>`. Keep the three folders side by side.

- `release-ui` uses `vdp-core`, `vdp-secrets`, `vdp-dbt`, `vdp-git`, `vdp-jira`, `vdp-release` and `vdp-cli` (with
  `default-features = false`, enabling `sources`, `release`, `sql`, `qa`). `vaultspeed-framework` adds `vdp-db` and `vdp-sql-script`
  and depends on `release-ui` as well.
- From `vdp-cli` they use `args::{GlobalArgs, Command, ReleaseCommand}`, `bootstrap::{context, adbc_driver, resolve_secrets_from,
  SecretSources, ResolvedSecrets}`, `commands::{run_command, run_dbt_validation}` and `runtime::{run, exit_code, ...}`; they build
  their own command line on `Command` and call `run_command` for everything they do not handle.
- A public API change here (a type, a trait method, a function signature, a feature) can break them without the image build noticing.
  Build both after such a change:
  ```bash
  (cd ../release-ui && cargo build --locked --workspace)
  (cd ../vaultspeed-framework && cargo build --locked --workspace)
  ```
  Their `Cargo.lock` files record this workspace's dependencies too, so adding or bumping a dependency here changes the lock files of
  both siblings (run the build without `--locked` once in each, commit the lock changes).
- Hand-off at run time: `vdp vaultspeed ...` and `vdp release-ui ...` replace the `vdp` process with `vdp-vaultspeed` or
  `vdp-release-ui` (`external::HANDOFFS`), looked up next to `vdp`, then on `PATH`. The same `vdp.toml` serves all binaries, which is why
  `vaultspeed`, `vaultspeed_release`, `release_hub` and `notify` are known sections here.
- Build them by hand: `cargo build --release --locked -p vdp-release-ui-cli` in `src/release-ui`, `cargo build --release --locked -p
  vdp-vaultspeed-cli` in `src/vaultspeed-framework`; put the binaries next to `vdp` or on `PATH`.

## Bumping the Rust toolchain

The channel is `1.96` everywhere; change them together:
1. `rust-toolchain.toml` in `src/vdp-rust-utils`, `src/release-ui` and `src/vaultspeed-framework`; `rust-version` under
   `[workspace.package]` in each `Cargo.toml`.
2. `ARG RUST_VERSION` in the dev image `Dockerfile` (the build stage is `rust:${RUST_VERSION}-trixie`).
3. The dbt fork pins its own channel in `infrastructure/dbt-singlestore-base-image/src/dbt/rust-toolchain.toml` (also `1.96` now); the
   comments in the three `rust-toolchain.toml` say the channels are meant to match.
4. Documentation that names the version: `infrastructure/dbt-singlestore-dev-image/README.md`; `README.md` and `CLAUDE.md` of
   `src/vdp-rust-utils`, `src/release-ui` and `src/vaultspeed-framework`; in `src/vdp-rust-utils` also this skill's `SKILL.md`,
   `references/testing.md` and this file; `.agents/skills/vdab-dbt-workshop-architecture/SKILL.md` (section 12).
   `grep -rn "1\.96" --include='*.md' .` from the repository root lists them; the sibling skills may name it too.
5. Run the gate (`testing.md`), build the siblings, then build the image.

## Prebuilt binary and local copies

`vdp` in the dev image folder (gitignored) is an optional prebuilt release binary of `vdp-cli`; it wins over the source build.
Make it with `cargo build --release --locked -p vdp-cli` in this workspace and `cp target/release/vdp ../../vdp`. It must run on the
base image's glibc (Debian trixie): the build stage uses `rust:1.96-trixie` for that reason, a host build on a newer system may not
run. A stale prebuilt wins silently, `docker build` never rebuilds from source while the file exists: delete or refresh it after
source changes. A new image is not picked up by a running devcontainer: rebuild or recreate the container (VS Code: Dev Containers:
Rebuild Container). The folder `local/` (gitignored, not in the image) keeps the untouched originals of
the vdp source and a binary built from it; they describe a larger code base than this tree.
