#!/bin/sh
# Provisions the workshop Vault. Repeatable: every step checks first and changes only what is missing or different, so it
# can run on every start and again by hand:
#   docker exec workshop-vault sh /vault/workshop/provision.sh
#
# What it sets up (see README.md next to this file for the why):
#   - AppRole auth, and the KV v2 mount `kv-workshop`
#   - per environment (dev, val, prd): kv-workshop/vdp/<env>/dbt with SINGLESTORE_PASS. This is the path `vdp` reads
#     ([secrets].precedence has `vault:vdp/{env_short}/dbt`); it is the one path every developer account can read.
#   - per developer account: kv-workshop/users/<account> with SINGLESTORE_USER and SINGLESTORE_PASS, readable by that
#     developer's AppRole only
#   - one read-only policy and one AppRole per developer account, with a fixed role id and a fixed secret id
#
# All values are workshop fixtures for a local practice stack. They arrive as environment variables from
# infrastructure/compose/docker-compose.yml, which is also where the overview page reads them from.
set -eu

export VAULT_ADDR="${VAULT_ADDR:-http://127.0.0.1:8200}"
export VAULT_TOKEN="${VAULT_DEV_ROOT_TOKEN_ID:?VAULT_DEV_ROOT_TOKEN_ID is not set}"

MOUNT="${WORKSHOP_KV_MOUNT:-kv-workshop}"
ACCOUNTS="${WORKSHOP_DEVELOPERS:-pu_dev1 pu_dev2 pu_dev3}"
ENVIRONMENTS="${WORKSHOP_ENVIRONMENTS:-dev val prd}"
# The password of the developer accounts in SingleStore (infrastructure/singlestore/init_global.sql: the same for all).
DB_PASSWORD="${WORKSHOP_DEVELOPER_DB_PASSWORD:?WORKSHOP_DEVELOPER_DB_PASSWORD is not set}"

log() { echo "vault-provision: $*"; }

# The server is started next to this script: wait until it answers and is unsealed (dev mode is).
tries=0
until vault status >/dev/null 2>&1; do
  tries=$((tries + 1))
  [ "$tries" -le 120 ] || { log "server did not come up"; exit 1; }
  sleep 0.5
done

# Not ready until provisioning has finished again (the healthcheck looks at this file).
rm -f /tmp/provisioned

# ---- engines -----------------------------------------------------------------------------------------------------
vault auth list -format=json | grep -q '"approle/"' || vault auth enable approle >/dev/null
vault secrets list -format=json | grep -q "\"$MOUNT/\"" || vault secrets enable -path="$MOUNT" -version=2 kv >/dev/null

# Writes a KV v2 secret only when it differs from what is stored (a plain `kv put` makes a new version every start).
# Usage: put_secret <path> KEY=value ...   Values are compared key by key, so extra keys do not matter.
put_secret() {
  path="$1"; shift
  current="$(vault kv get -mount="$MOUNT" -format=json "$path" 2>/dev/null || true)"
  same=yes
  for pair in "$@"; do
    key="${pair%%=*}"; value="${pair#*=}"
    # the value is looked up as a JSON string: no jq in the image, and the fixtures contain no quotes or backslashes
    echo "$current" | grep -q "\"$key\": *\"$value\"" || same=no
  done
  if [ "$same" = yes ]; then return 0; fi
  vault kv put -mount="$MOUNT" "$path" "$@" >/dev/null
  log "wrote $MOUNT/$path"
}

# ---- secrets read by vdp: one path per environment --------------------------------------------------------------
# vdp resolves `vault:vdp/{env_short}/dbt` against the environment it runs for, but cannot put the user name into the
# path, so this path holds what every developer account shares: the password (the user name comes from .devcontainer/.env).
for env in $ENVIRONMENTS; do
  put_secret "vdp/$env/dbt" "SINGLESTORE_PASS=$DB_PASSWORD"
done

# ---- policies, AppRoles, personal secrets ---------------------------------------------------------------------
for account in $ACCOUNTS; do
  upper="$(echo "$account" | tr '[:lower:]' '[:upper:]')"
  eval "role_id=\${WORKSHOP_ROLE_ID_$upper:?WORKSHOP_ROLE_ID_$upper is not set}"
  eval "secret_id=\${WORKSHOP_SECRET_ID_$upper:?WORKSHOP_SECRET_ID_$upper is not set}"

  put_secret "users/$account" "SINGLESTORE_USER=$account" "SINGLESTORE_PASS=$DB_PASSWORD"

  # Read-only, nothing else: no list, no write, no other account's record.
  {
    for env in $ENVIRONMENTS; do
      printf 'path "%s/data/vdp/%s/dbt" {\n  capabilities = ["read"]\n}\n' "$MOUNT" "$env"
    done
    printf 'path "%s/data/users/%s" {\n  capabilities = ["read"]\n}\n' "$MOUNT" "$account"
  } | vault policy write "workshop-$account" - >/dev/null

  # Tokens live one hour and may be used any number of times (vdp logs in per command and revokes right after reading).
  vault write "auth/approle/role/$account" \
    token_policies="workshop-$account" token_ttl=1h token_max_ttl=4h token_num_uses=0 \
    secret_id_ttl=0 secret_id_num_uses=0 bind_secret_id=true >/dev/null
  vault write "auth/approle/role/$account/role-id" role_id="$role_id" >/dev/null

  # A custom secret id can be created once (a second time is an error). The lookup of an unknown id succeeds with an
  # empty answer, so look for the accessor in the answer.
  if ! vault write -format=json "auth/approle/role/$account/secret-id/lookup" secret_id="$secret_id" 2>/dev/null \
      | grep -q '"secret_id_accessor"'; then
    vault write "auth/approle/role/$account/custom-secret-id" secret_id="$secret_id" >/dev/null
    log "created the secret id of $account"
  fi
done

touch /tmp/provisioned
log "ready: mount $MOUNT, AppRoles: $ACCOUNTS"
