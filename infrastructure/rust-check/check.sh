#!/bin/sh
# The quality gate of one Cargo workspace. Runs in the workspace folder (/src/<name>); called by the root Makefile
# (`make rust-check`, `make rust-test`), never by hand.
#   check         fmt, clippy (vdp-rust-utils: also per cargo feature), tests, docs
#   test [args]   only the tests; the arguments go to `cargo test`
set -eu

ws=$(basename "$(pwd)")

run() {
    printf '\n--- %s: %s\n' "$ws" "$*"
    "$@"
}

# Without arguments: all tests of the workspace. With arguments (`-p crate`, `--test name`, a filter): they pick the tests
# themselves; `--workspace` would fight `-p`, and in these virtual workspaces plain `cargo test` already covers every crate.
tests() {
    if [ "$#" -eq 0 ]; then
        run cargo test --workspace --locked
    else
        run cargo test --locked "$@"
    fi
}

case "${1:-}" in
check)
    run cargo fmt --all --check
    run cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    if [ "$ws" = vdp-rust-utils ]; then
        # every module is a cargo feature of vdp-cli: each one alone, and none
        run cargo clippy -p vdp-cli --all-targets --no-default-features --locked -- -D warnings
        for feature in sources release sql qa; do
            run cargo clippy -p vdp-cli --all-targets --no-default-features --features "$feature" --locked -- -D warnings
        done
    fi
    tests
    printf '\n--- %s: RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --all-features --locked\n' "$ws"
    RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps --all-features --locked
    ;;
test)
    shift
    tests "$@"
    ;;
*)
    echo "usage: check.sh check | test [cargo test arguments]" >&2
    exit 2
    ;;
esac
