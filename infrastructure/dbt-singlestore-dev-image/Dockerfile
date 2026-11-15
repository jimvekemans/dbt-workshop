# syntax=docker/dockerfile:1.7
# Developer image for the VS Code devcontainer: the base image (Fusion dbt + SingleStore ADBC driver) plus the `vdp`
# toolkit (which fronts `dbt`), sqlfmt, dbt-bouncer and pre-commit.
#
# Everything comes from this directory, like the base image:
#   src/vdp-rust-utils  the vdp toolkit; the prebuilt ./vdp (release binary) is used as is when present
#   src/sqlfmt          sqlfmt fork with SingleStore `:>` casts
#   src/dbt-bouncer     dbt-bouncer fork
#   docker build -t dbt-singlestore-dev:1.11.11 .
ARG BASE_IMAGE=dbt-singlestore-base:1.11.11
ARG RUST_VERSION=1.96

# ---------------------------------------------------------------------------------------------------------------------
# vdp: prebuilt ./vdp, or `cargo build --release --locked -p vdp-cli` from src/vdp-rust-utils
# trixie matches the base image's glibc, so the binary built here always runs there.
FROM rust:${RUST_VERSION}-trixie AS vdp-build
# The release compile is memory-hungry; lower this (docker build --build-arg CARGO_BUILD_JOBS=2) on a busy host.
ARG CARGO_BUILD_JOBS=
# The Dockerfile is listed so the COPY never matches nothing when ./vdp is absent.
COPY Dockerfile vdp* /prebuilt/
# The whole src/ folder is mounted, not src/vdp-rust-utils: a mount of a folder that is not there fails the build,
# and a clone of the repository has no vdp source (see README.md). Then the image is built without vdp.
RUN --mount=type=bind,source=src,target=/src-all \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/cargo-target-vdp \
    set -eux; mkdir -p /out; \
    if [ -f /prebuilt/vdp ]; then cp /prebuilt/vdp /out/vdp; exit 0; fi; \
    if [ ! -f /src-all/vdp-rust-utils/Cargo.toml ]; then \
        echo "No prebuilt ./vdp and no src/vdp-rust-utils: building the image without the vdp toolkit" >&2; exit 0; fi; \
    if [ -n "$CARGO_BUILD_JOBS" ]; then export CARGO_BUILD_JOBS; else unset CARGO_BUILD_JOBS; fi; \
    cd /src-all/vdp-rust-utils; \
    CARGO_TARGET_DIR=/cargo-target-vdp cargo build --release --locked -p vdp-cli; \
    cp /cargo-target-vdp/release/vdp /out/vdp

# ---------------------------------------------------------------------------------------------------------------------
FROM ${BASE_IMAGE}
USER root
ENV DEBIAN_FRONTEND=noninteractive
# Python for the formatting and lint tools (Debian packages; dnf fallback only so a UBI-based base also builds).
RUN if command -v apt-get >/dev/null; then \
        apt-get update && apt-get install -y --no-install-recommends python3 python3-venv python3-pip make \
        && rm -rf /var/lib/apt/lists/*; \
    else dnf -y install python3 python3-pip make && dnf clean all; fi

# One venv with sqlfmt (SingleStore `:>` cast fork, with its jinja formatting extra), dbt-bouncer and pre-commit.
RUN --mount=type=bind,source=src/sqlfmt,target=/tools/sqlfmt,rw \
    --mount=type=bind,source=src/dbt-bouncer,target=/tools/dbt-bouncer,rw \
    --mount=type=cache,target=/root/.cache/pip \
    set -eux; python3 -m venv /opt/devtools; \
    /opt/devtools/bin/pip install "/tools/sqlfmt[jinjafmt]" /tools/dbt-bouncer pre-commit; \
    for b in sqlfmt dbt-bouncer pre-commit; do ln -s "/opt/devtools/bin/$b" "/usr/local/bin/$b"; done

# `dbt` becomes `vdp`, which runs the real engine from libexec. The engine still finds its driver in the lib/dbt/
# directory above it (/usr/local/lib/dbt/). The base may already ship this layout; then only vdp is replaced.
# Without vdp (no prebuilt binary and no source) `dbt` stays the plain engine and everything else works the same.
COPY --from=vdp-build /out/ /tmp/vdp-out/
RUN set -eux; \
    if [ -f /tmp/vdp-out/vdp ]; then \
        install -m 755 /tmp/vdp-out/vdp /usr/local/bin/vdp; \
        if [ ! -L /usr/local/bin/dbt ]; then \
            mkdir -p /usr/local/libexec/dbt; mv /usr/local/bin/dbt /usr/local/libexec/dbt/dbt; \
            ln -s vdp /usr/local/bin/dbt; fi; \
    else echo "vdp is not in this image: dbt is the plain engine"; fi; \
    rm -rf /tmp/vdp-out
ENV VDP_DBT_BIN=/usr/local/libexec/dbt/dbt \
    DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true \
    DBT_SEND_ANONYMOUS_USAGE_STATS=false \
    DO_NOT_TRACK=1

USER dbt:root
WORKDIR /usr/app/dbt/workspace
CMD ["dbt", "--version"]
