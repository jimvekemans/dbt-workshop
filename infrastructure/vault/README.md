# Workshop Vault

A local stand-in for a secrets vault: HashiCorp Vault in **dev mode** (in memory, unsealed, plain HTTP). It exists to
prove one thing in a demo: the developer's database password is not in a file on the laptop; the toolbox (`vdp`,
installed as `dbt`) fetches it from the vault when a command starts. Besides developers who choose the vault way, only the
vault stage of the Studio flow `developers-toolbox` (`toolbox.run` with `auth: vault`) uses it; the rest of the stack
works with the container stopped.

| | |
| :-- | :-- |
| Container / host name | `workshop-vault` / `vault`, `http://vault:8200` on `workshop_net` |
| From the host | `http://127.0.0.1:8200` (`WORKSHOP_VAULT_PORT`), UI at `/ui` (method Token) |
| Root token | `workshop-root-token` |
| Files | `entrypoint.sh` (starts the server, provisions in the background), `provision.sh` (idempotent) |

Dev mode forgets everything when the container stops, so **every start provisions again** (also after
`docker restart workshop-vault`). The container is healthy once provisioning has finished. To run it by hand:
`docker exec workshop-vault sh /vault/workshop/provision.sh` (changes nothing when everything is in place).

## What is in it (workshop fixtures)

KV v2 mount `kv-workshop`:

| Path | Keys | Read by |
| :-- | :-- | :-- |
| `vdp/dev/dbt`, `vdp/val/dbt`, `vdp/prd/dbt` | `SINGLESTORE_PASS` | the three developer AppRoles (what `vdp` reads) |
| `users/pu_dev1`, `users/pu_dev2`, `users/pu_dev3` | `SINGLESTORE_USER`, `SINGLESTORE_PASS` | that developer's AppRole only |

One AppRole per developer account, policy `workshop-<account>` (read only, nothing else), fixed ids:

| Account | Role id | Secret id |
| :-- | :-- | :-- |
| `pu_dev1` | `workshop-role-id-pu_dev1` | `workshop-secret-id-pu_dev1` |
| `pu_dev2` | `workshop-role-id-pu_dev2` | `workshop-secret-id-pu_dev2` |
| `pu_dev3` | `workshop-role-id-pu_dev3` | `workshop-secret-id-pu_dev3` |

The password is the one of `pu_dev1..3` in SingleStore (`WorkshopPwd2026!`, `infrastructure/singlestore/init_global.sql`);
the values live in `docker-compose.yml` (service `vault`), which is where the overview page reads them from.

## How `vdp` uses it

`dbt-singlestore-workshop/vdp.toml` lists `vault:vdp/{env_short}/dbt` first in `[secrets].precedence`. `vdp` reads
exactly one path per environment, and only `{env}` and `{env_short}` can appear in it: it cannot put the user name in the
path. So three developers cannot have three different values at the path `vdp` reads. What differs per developer is the
identity (one AppRole each: separate login, revocable, own audit trail) and the personal record `users/<account>`, which
no other developer can read. Because the workshop gives `pu_dev1..3` the same password, the shared path serves all
three; the user name comes from `.devcontainer/.env`. Truly different passwords per developer need a `{user}`
placeholder in `vdp` (path `vdp/{env_short}/users/{user}`) first; the records are already laid out for it.

`vdp` uses Vault only when `KEYVAULT_ROLE_ID` and `KEYVAULT_SECRET_ID` are set (in `.devcontainer/.env` or the process
environment); without them the vault is never contacted. Precedence: Vault, then `.devcontainer/.env`, then the process
environment.

## Check it with curl

```sh
TOKEN=$(curl -s -d '{"role_id":"workshop-role-id-pu_dev1","secret_id":"workshop-secret-id-pu_dev1"}' \
  http://127.0.0.1:8200/v1/auth/approle/login | sed -n 's/.*"client_token":"\([^"]*\)".*/\1/p')
curl -s -H "X-Vault-Token: $TOKEN" http://127.0.0.1:8200/v1/kv-workshop/data/vdp/dev/dbt      # 200
curl -s -H "X-Vault-Token: $TOKEN" http://127.0.0.1:8200/v1/kv-workshop/data/users/pu_dev2    # permission denied
```
