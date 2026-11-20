# syntax=docker/dockerfile:1.7
# Toolchain image of `make rust-check`, `rust-test` and `rust-cargo`: the quality gate of the three Cargo workspaces under
# infrastructure/dbt-singlestore-dev-image/src/ runs here, so the host needs no Rust. The Makefile passes RUST_VERSION,
# which it reads from the dev image Dockerfile (`ARG RUST_VERSION`), so the version lives in one place.
ARG RUST_VERSION
FROM rust:${RUST_VERSION}-trixie
ARG RUST_VERSION

# The workspaces pin `channel = "1.96"` in rust-toolchain.toml. The official image names its toolchain after the patch
# release (1.96.x), which rustup does not count as a match: it would download the toolchain again on every run.
# Installing the channel under its own name, with the components the gate needs, makes the pin a match.
RUN rustup toolchain install "${RUST_VERSION}" --profile minimal --component rustfmt --component clippy \
 && rustup default "${RUST_VERSION}"

# The container runs as the host user, so the cache folders must be writable for any uid; the named volumes the Makefile
# mounts there copy these permissions when they are first created.
RUN mkdir -p /usr/local/cargo/registry /usr/local/cargo/git /cargo-target \
 && chmod a+w /usr/local/cargo/registry /usr/local/cargo/git /cargo-target

COPY check.sh /usr/local/bin/rust-check
# Build output goes to a volume, never into the mounted sources. The gate never debugs, and debug info made the build
# output of vaultspeed-framework (DuckDB) over 20 GB; without it the dev and test profiles stay far smaller and compile faster.
ENV CARGO_TARGET_DIR=/cargo-target HOME=/tmp CARGO_PROFILE_DEV_DEBUG=0
