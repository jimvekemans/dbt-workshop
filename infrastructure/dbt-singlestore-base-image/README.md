# dbt-singlestore base image

Base image for every dbt invocation in the workshop. The dev image (devcontainer) and the `dbt-vdp` project image
(GitLab CI jobs, Airflow tasks) are built on it. It holds two native artifacts and nothing else of note (no Python, no dbt-core):

| Artifact | In the image | Source |
|---|---|---|
| `dbt`: Fusion dbt with the SingleStore adapter | `/usr/local/bin/dbt` | [`jimvekemans/dbt`](https://github.com/jimvekemans/dbt), branch `feat/backfill-hardening-pr`, submodule `src/dbt` |
| `libadbc_driver_singlestore.so`: SingleStore ADBC driver | `/usr/local/lib/dbt/` | [`singlestore-labs/singlestore-adbc-connector`](https://github.com/singlestore-labs/singlestore-adbc-connector), branch `main`, submodule `src/singlestore-adbc-connector` |

dbt finds the driver by walking up from its own executable to a `lib/dbt/` directory, so the two paths belong together.
Without the driver, dbt can't connect to SingleStore. The image also sets `DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true`,
because this build refuses a `singlestore` profile without it.

## Layout

```
dbt-singlestore-base-image/
├── Dockerfile                      3 stages: dbt-build, adbc-build, runtime
├── .dockerignore                   keeps src/*/.git and src/*/target out of the context
├── dbt                             optional prebuilt binary (gitignored)
├── libadbc_driver_singlestore.so   optional prebuilt driver (gitignored)
└── src/
    ├── dbt/                        git submodule
    └── singlestore-adbc-connector/ git submodule
```

## Building

```bash
docker build -t dbt-singlestore-base:1.11.11 infrastructure/dbt-singlestore-base-image
```

Normally you don't run this yourself: `make images` and `make bootstrap` at the repository root build it. The
Dockerfile does not rewrite git URLs.

For each artifact, the build **uses the prebuilt file in this folder if it exists, and otherwise builds it from its
submodule**:

- **dbt**: `cargo build --release -p dbt-sa-cli` (the crate's binary is named `dbt`). It uses the Rust channel from
  `src/dbt/rust-toolchain.toml`, installed with the minimal profile. The release build is required, because a debug
  build fails at runtime. A cold build takes a long time and a lot of memory. On a busy host, run
  `--build-arg CARGO_BUILD_JOBS=2` or stop `workshop-gitlab` and `workshop-singlestore` first.
- **Driver**: `go build -tags driverlib -buildmode=c-shared ./pkg` in `src/singlestore-adbc-connector/go`, using the Go
  version from that module's `go.mod`. It takes about a minute.

Cargo and Go caches live in BuildKit cache mounts, so rebuilds are incremental.

### First checkout

Without prebuilt binaries, fetch the submodules first, or the build stops and tells you to:

```bash
git submodule update --init
```

### Forcing a rebuild from source

Delete (or move away) the prebuilt file, then build again:

```bash
rm infrastructure/dbt-singlestore-base-image/dbt
```

### Updating the sources

```bash
git submodule update --remote infrastructure/dbt-singlestore-base-image/src/dbt
```

Commit the new submodule pointer, then remove the stale prebuilt binary so the next build picks up the change.

### Pulling a built binary out of the image

To keep a freshly built binary as a local prebuilt and skip the compile on later builds:

```bash
docker build --target dbt-build -t dbt-build infrastructure/dbt-singlestore-base-image
```

```bash
docker run --rm -v "$PWD/infrastructure/dbt-singlestore-base-image:/x" dbt-build cp /out/dbt /x/dbt
```

Use `--target adbc-build` and `/out/libadbc_driver_singlestore.so` for the driver.

## Checking an image

```bash
docker run --rm dbt-singlestore-base:1.11.11 dbt --version
```
