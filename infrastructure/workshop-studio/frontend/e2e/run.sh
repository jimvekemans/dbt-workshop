#!/usr/bin/env bash
# One command: ./e2e/run.sh   (see README.md). Needs only docker; nothing is installed on the host.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PW_VERSION="${PW_VERSION:-1.56.0}"
OUT="${OUT:-$PWD/review}"
mkdir -p "$OUT"
docker run --rm --network host --ipc=host \
  -v "$HERE":/e2e:ro -v "$OUT":/out \
  -e BASE_URL="${BASE_URL:-http://127.0.0.1:5199/?mock}" \
  -e SCENARIOS="${SCENARIOS:-}" -e NO_TAB="${NO_TAB:-}" -e SKIP_SWEEP="${SKIP_SWEEP:-}" -e WORKERS="${WORKERS:-4}" \
  -e PW_VERSION -e AXE_VERSION="${AXE_VERSION:-4.10.2}" -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  "mcr.microsoft.com/playwright:v${PW_VERSION}-noble" bash /e2e/inside.sh
