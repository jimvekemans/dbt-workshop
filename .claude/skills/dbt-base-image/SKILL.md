---
name: dbt-base-image
description: >-
  Inner workings of the workshop's dbt-singlestore base image (infrastructure/dbt-singlestore-base-image): the three-stage
  Dockerfile that uses a prebuilt Fusion dbt binary and SingleStore ADBC driver when present and otherwise builds them
  from git submodules (src/dbt = jimvekemans/dbt feat/backfill-hardening-pr, src/singlestore-adbc-connector =
  singlestore-labs/singlestore-adbc-connector main), driver discovery via lib/dbt/, DBT_ALLOW_EXPERIMENTAL_ADAPTERS,
  image tags and consumers (including the dev image and the dbt-vdp project image built on it), and build pitfalls. Use when building, rebuilding, debugging or changing the base image, the
  dbt binary, the ADBC driver, the submodules, or when dbt in CI/Airflow/devcontainer fails to start or reach SingleStore.
---

# dbt-singlestore base image

Folder: `infrastructure/dbt-singlestore-base-image/`. User-facing docs are in its `README.md`. This skill covers the
details that matter when you change or debug the image.

## 1. What the image is

- `debian:trixie-slim` with bash, git, curl, sudo, rsync, openssh-client and ca-certificates, plus two native
  artifacts:
  - `/usr/local/bin/dbt`: Fusion dbt (Rust) with the SingleStore adapter. `dbt --version` prints `dbt-oss 2.x`.
  - `/usr/local/lib/dbt/libadbc_driver_singlestore.so`: a Go cgo c-shared ADBC driver that exports `AdbcDriverInit`.
- There is no Python and no dbt-core. Adapter macros are compiled into the binary.
- **Driver discovery:** dbt never downloads the driver. It walks up from its executable's directory looking for
  `lib/dbt/`, so `/usr/local/bin/dbt` finds `/usr/local/lib/dbt/`. If you move either file, keep that relationship
  or dbt can't open a SingleStore connection.
- `ENV DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true`. Without it, the build rejects the profile with "The 'singlestore'
  adapter is not yet supported by dbt".
- User `dbt` (uid 1000, gid 1000, also in `wheel`, passwordless sudo) and workdir `/usr/app/dbt/workspace`. Git has
  `safe.directory '*'` set. The Dockerfile does not rewrite any git URLs: git package URLs in a dbt project must resolve
  as written.
- Two images are built on top of it: the **dev image** `dbt-singlestore-dev:1.11.11`
  (`infrastructure/dbt-singlestore-dev-image/`, adds `vdp`, `sqlfmt`, `dbt-bouncer`, `pre-commit`; built by
  `make images` from its own `src/` and optional prebuilt `./vdp`) and the **project image** `dbt-vdp:<git tag>` (`dbt-singlestore-workshop/Dockerfile`,
  built by CI).

## 2. Dockerfile stages

| Stage | Output | Rule |
|---|---|---|
| `dbt-build` | `/out/dbt` | Use `./dbt` if present, else build it from `src/dbt` |
| `adbc-build` | `/out/libadbc_driver_singlestore.so` | Use `./libadbc_driver_singlestore.so` if present, else build it from `src/singlestore-adbc-connector` |
| final (unnamed) | runtime image | `COPY --from=` both stages into the paths above |

How the "use it if present" check works, which is easy to break:

- `COPY Dockerfile dbt* /prebuilt/`: the glob matches nothing when the binary is absent. Listing `Dockerfile` as
  well guarantees the COPY never has zero sources, because that's an error. Don't rename the submodule dirs to
  anything starting with `dbt` or `libadbc` at the context root, or the glob picks them up. That's why they live
  under `src/`.
- The sources come in through `RUN --mount=type=bind,source=src/...` (read-only), not `COPY`, so a prebuilt build
  doesn't add the source tree to a layer. Cargo writes to `CARGO_TARGET_DIR=/cargo-target`, a cache mount, because
  the bind mount is read-only. The binary is copied to `/out` because cache-mount contents never end up in the
  image.
- If the prebuilt file is missing and the submodule dir is empty (not initialized), the step fails with a message
  saying to run `git submodule update --init`.
- Toolchains are installed inside the `else` branch only, so a prebuilt build stays fast and pulls nothing extra.
- Both builder stages share the same Debian base as the runtime image (trixie), so glibc matches.

### dbt build details

- `cargo build --release -p dbt-sa-cli`. The `[[bin]]` in `crates/dbt-sa-cli/Cargo.toml` is named `dbt`. The fork
  has no `Cargo.lock`, so don't add `--locked`.
- Rust: rustup with `--profile minimal` on the channel parsed from `src/dbt/rust-toolchain.toml`, used through
  `RUSTUP_TOOLCHAIN`. This deliberately bypasses the toolchain file's 7 cross targets and its
  clippy/rustfmt/rust-analyzer components, which rustup would otherwise install.
- apt deps: build-essential, pkg-config, libssl-dev, cmake, clang, protobuf-compiler, libprotobuf-dev. The
  protobuf packages are required.
- It must be a **release** build (~200 MB). A debug build compiles but fails at runtime with "Missing
  dbt_project.yml in embedded package 'dbt-adapters'". To check a binary:
  `strings dbt | grep -c /target/debug/` should print 0.
- The `embed-ui` default feature needs no Node: `crates/dbt-docs-server/web/dist` is committed.
- Memory: `--build-arg CARGO_BUILD_JOBS=2` limits parallelism. Otherwise stop `workshop-gitlab` and
  `workshop-singlestore` while compiling. An OOM kill in the middle of a build shows up as an rustc/linker error, not
  a clear message.

### Driver build details

- `cd go && CGO_ENABLED=1 go build -tags driverlib -trimpath -buildvcs=false -buildmode=c-shared -ldflags "-s -w"
  -o /out/libadbc_driver_singlestore.so ./pkg`. The `driverlib` build tag is mandatory: `pkg/driver.go` has
  `//go:build driverlib`, and without the tag Go reports "build constraints exclude all Go files". The generated `.h`
  is deleted.
- Go comes from the go.dev tarball for the exact version on the `go` line of `go/go.mod`, matched to
  `dpkg --print-architecture`.
- Upstream's official route is `pixi run make` (adbc-drivers/dev's `adbc-make`), which needs pixi and Python. The
  plain `go build` produces an equivalent shared library (verified: about 39 MB, loads fine) without either.

## 3. Submodules

Paths are relative to the repo root. `.gitmodules` records the branches, so `git submodule update --remote` follows
them.

- `infrastructure/dbt-singlestore-base-image/src/dbt`: `https://github.com/jimvekemans/dbt.git`, branch
  `feat/backfill-hardening-pr`.
- `infrastructure/dbt-singlestore-base-image/src/singlestore-adbc-connector`:
  `https://github.com/singlestore-labs/singlestore-adbc-connector.git`, branch `main`. There's no repo called
  `singlestore-adbc-driver`. Earlier binaries were built from a `feat/singlestore-improvements` branch; if the
  `main` driver behaves differently, that's the first thing to check.
- `.dockerignore` excludes `src/*/.git`, `src/*/target` and `node_modules`, which keeps the context small (a submodule's
  `.git` is just a pointer file anyway).
- The prebuilt `dbt` and `libadbc_driver_singlestore.so` are gitignored in the root `.gitignore`. After you bump a
  submodule, **delete the matching prebuilt file**, or the image keeps shipping the old binary.

## 4. Who builds and consumes it

- `make images` and `bootstrap_stack.py::build_images()` run `docker build -t dbt-singlestore-base:1.11.11 <this folder>`.
  The tag is not changed or pushed. `bootstrap_stack.py::register_runner()` registers the GitLab runner with
  `--docker-image dbt-singlestore-base:1.11.11` and `--docker-pull-policy if-not-present`.
- Consumers: `dbt-singlestore-workshop/.gitlab-ci.yml` (default job image), `dbt-singlestore-workshop/Dockerfile`
  (`FROM`, argument `BASE_IMAGE`), `infrastructure/dbt-singlestore-dev-image/Dockerfile` (`FROM`), and through the project
  image the Airflow dbt tasks. See the `vdab-dbt-workshop-architecture` skill in `.agents/skills/` for the wider stack.
- After rebuilding the base image, rebuild the dev image and let CI build a new `dbt-vdp` tag, or they keep the old layers.

## 5. Quick checks

```bash
docker run --rm dbt-singlestore-base:1.11.11 dbt --version
docker run --rm dbt-singlestore-base:1.11.11 ls -la /usr/local/lib/dbt/
docker run --rm --entrypoint sh dbt-singlestore-base:1.11.11 -c 'echo $DBT_ALLOW_EXPERIMENTAL_ADAPTERS'
nm -D --defined-only libadbc_driver_singlestore.so | grep AdbcDriverInit
```

To build only one artifact: `docker build --target dbt-build` or `--target adbc-build`. The result is in `/out/`
inside that stage image.
