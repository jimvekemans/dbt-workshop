#!/usr/bin/env bash
# Build/test helper for hosts where the project lives on a filesystem without symlink support (e.g. exFAT):
# copies the sources to a scratch dir with a normal filesystem, installs there and runs the same npm scripts.
set -euo pipefail
SRC="$(cd "$(dirname "$0")" && pwd)"
WORK="${1:-${TMPDIR:-/tmp}/workshop-studio-fe}"
mkdir -p "$WORK"
rsync -a --delete --exclude node_modules --exclude dist --exclude build-check.sh "$SRC"/ "$WORK"/
cd "$WORK"
[ -d node_modules ] || npm ci --no-audit --no-fund   # exact versions from package-lock.json
npm run build
npm test
echo "dist built in $WORK/dist"
