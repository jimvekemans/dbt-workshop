#!/usr/bin/env bash
mkdir -p /tmp/pw && cd /tmp/pw || exit 1
[ -f package.json ] || npm init -y >/dev/null
npm i --no-audit --no-fund --silent "playwright@${PW_VERSION}" "axe-core@${AXE_VERSION}" || exit 1
cp /e2e/capture.mjs .
node capture.mjs; rc=$?
chown -R "$HOST_UID:$HOST_GID" /out 2>/dev/null
exit $rc
