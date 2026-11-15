# syntax=docker/dockerfile:1.7
# Runtime image around the Rust Fusion dbt binary (no Python at all).
#
# The dbt binary and the SingleStore ADBC driver are taken from this directory when present (prebuilt, gitignored).
# When either is missing, it is built from its git submodule under src/:
#   src/dbt                         github jimvekemans/dbt, branch feat/backfill-hardening-pr
#   src/singlestore-adbc-connector  github singlestore-labs/singlestore-adbc-connector, branch main
# Run `git submodule update --init` first if you have no prebuilt binaries.

# ---------------------------------------------------------------------------------------------------------------------
# dbt: prebuilt ./dbt, or `cargo build --release -p dbt-sa-cli` from src/dbt
FROM debian:trixie-slim AS dbt-build
ENV DEBIAN_FRONTEND=noninteractive
# The release compile is memory-hungry; lower this (docker build --build-arg CARGO_BUILD_JOBS=2) on a busy host.
ARG CARGO_BUILD_JOBS=
# The Dockerfile is listed so the COPY never matches nothing when ./dbt is absent.
COPY Dockerfile dbt* /prebuilt/
# rw: the fork has no Cargo.lock in git, so cargo writes one (into the build's own copy; nothing reaches the host)
RUN --mount=type=bind,source=src/dbt,target=/src/dbt,rw \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/cargo-target \
    set -eux; mkdir -p /out; \
    if [ -f /prebuilt/dbt ]; then cp /prebuilt/dbt /out/dbt; exit 0; fi; \
    if [ ! -f /src/dbt/Cargo.toml ]; then \
        echo "No prebuilt ./dbt and src/dbt is empty: run 'git submodule update --init'" >&2; exit 1; fi; \
    apt-get update; apt-get install -y --no-install-recommends \
        ca-certificates curl git build-essential pkg-config libssl-dev cmake clang protobuf-compiler libprotobuf-dev; \
    # Pin the channel from rust-toolchain.toml but skip its long target/component list (cross targets, rust-analyzer).
    RUST_CHANNEL="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' /src/dbt/rust-toolchain.toml)"; \
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "$RUST_CHANNEL"; \
    cd /src/dbt; \
    if [ -n "$CARGO_BUILD_JOBS" ]; then export CARGO_BUILD_JOBS; else unset CARGO_BUILD_JOBS; fi; \
    RUSTUP_TOOLCHAIN="$RUST_CHANNEL" CARGO_TARGET_DIR=/cargo-target /root/.cargo/bin/cargo build --release -p dbt-sa-cli; \
    cp /cargo-target/release/dbt /out/dbt

# ---------------------------------------------------------------------------------------------------------------------
# SingleStore ADBC driver: prebuilt ./libadbc_driver_singlestore.so, or a cgo c-shared build of
# src/singlestore-adbc-connector/go/pkg
FROM debian:trixie-slim AS adbc-build
ENV DEBIAN_FRONTEND=noninteractive
COPY Dockerfile libadbc_driver_singlestore.so* /prebuilt/
RUN --mount=type=bind,source=src/singlestore-adbc-connector,target=/src/adbc,rw \
    --mount=type=cache,target=/root/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    set -eux; mkdir -p /out; \
    if [ -f /prebuilt/libadbc_driver_singlestore.so ]; then \
        cp /prebuilt/libadbc_driver_singlestore.so /out/; exit 0; fi; \
    if [ ! -f /src/adbc/go/go.mod ]; then \
        echo "No prebuilt ./libadbc_driver_singlestore.so and src/singlestore-adbc-connector is empty: run 'git submodule update --init'" >&2; exit 1; fi; \
    apt-get update; apt-get install -y --no-install-recommends ca-certificates curl git build-essential; \
    GO_VERSION="$(sed -n 's/^go \([0-9.]*\)$/\1/p' /src/adbc/go/go.mod)"; \
    curl -sSfL "https://go.dev/dl/go${GO_VERSION}.linux-$(dpkg --print-architecture).tar.gz" | tar -C /usr/local -xz; \
    cd /src/adbc/go; \
    CGO_ENABLED=1 /usr/local/go/bin/go build -tags driverlib -trimpath -buildvcs=false -buildmode=c-shared -ldflags "-s -w" \
        -o /out/libadbc_driver_singlestore.so ./pkg; \
    rm -f /out/libadbc_driver_singlestore.h

# ---------------------------------------------------------------------------------------------------------------------
FROM debian:trixie-slim
USER root
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    bash git curl sudo rsync openssh-client ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Fusion dbt with the SingleStore adapter (github repo jimvekemans/dbt, branch feat/backfill-hardening-pr)
COPY --from=dbt-build /out/dbt /usr/local/bin/dbt
# SingleStore ADBC driver (github repo singlestore-labs/singlestore-adbc-connector, branch main).
# This dbt build loads SingleStore through it and never downloads it: dbt looks in the lib/dbt/ directory it finds
# walking up from its own executable, i.e. /usr/local/lib/dbt/ for /usr/local/bin/dbt.
COPY --from=adbc-build /out/libadbc_driver_singlestore.so /usr/local/lib/dbt/libadbc_driver_singlestore.so
RUN chmod 755 /usr/local/bin/dbt /usr/local/lib/dbt/libadbc_driver_singlestore.so && mkdir -p /usr/app/dbt/workspace
# This dbt build ships SingleStore as an experimental adapter and refuses the profile without this flag
# ("The 'singlestore' adapter is not yet supported by dbt"). Set in the image so CI, Airflow tasks and the bootstrap
# all have it.
ENV DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true

RUN groupadd -f -g 1000 vscode && groupadd -f wheel && \
    useradd -ms /bin/bash -u 1000 -g 1000 -d /usr/app/dbt dbt && \
    usermod -aG wheel dbt && \
    echo "dbt ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers && \
    git config --system --add safe.directory '*' && \
    chown -R 1000:0 /usr/app/dbt && chmod -R g+rwx /usr/app/dbt

USER dbt:root
RUN git config --global --add safe.directory '*'
WORKDIR /usr/app/dbt/workspace
CMD ["dbt", "--version"]
