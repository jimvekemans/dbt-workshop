#!/bin/sh
# Entrypoint of workshop-vault: provisioning in the background, the dev-mode server in the foreground.
#
# Dev mode keeps everything in memory, so every start (first start, `docker restart`, recreate) begins empty. That is why
# provisioning is part of the container's own start and not a one-shot companion service: a companion would not run
# again after `docker restart workshop-vault`. The server is started through the image's own entrypoint, which keeps
# the image behaviour (mlock capability check, running as the vault user, VAULT_DEV_ROOT_TOKEN_ID, listen address).
set -eu

sh /vault/workshop/provision.sh &

exec /usr/local/bin/docker-entrypoint.sh server -dev
