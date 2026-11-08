---
name: vdab-dbt-workshop-architecture
description: >-
  Architecture and runbook of the self-contained VDAB dbt-singlestore workshop stack at /home/jim/Documents/dbt-workshop:
  repo layout (infrastructure/ + dbt-singlestore-workshop/ + .agents/), the infrastructure/workshop.sh entry point and the
  manager container that runs all workshop Python (bootstrap, reset, trickle, faults, milestones), the workshop-*
  containers and their ports, Docker volumes (workshop_dev / workshop_prod workspaces, SingleStore data + milestones),
  SingleStore database layout, RBAC and SQL quirks, the Fusion dbt base image, Airflow 3.3.2 runner patterns, the
  drift audit and the test setup. Use it when starting, bootstrapping, resetting, syncing, debugging, testing or changing
  any part of the workshop stack.
---

# VDAB dbt-singlestore workshop: architecture & runbook

## 0. Ground rules
- **One entry point, no host Python**: everything goes through `infrastructure/workshop.sh` (only Docker + the compose
  plugin are needed on the host). Never `pip install` on the host or call `python bootstrap_stack.py` directly.
- **Self-contained**: every container is `workshop-<service>`, volumes are `workshop_*`, the network is `workshop_net`,
  compose project `dbt-workshop`. The user's personal SingleStore (container `singlestore`, host 3306, UI 8080, volume
  `singlestore-data`, `~/singlestore/docker-compose.yml`) belongs to other projects: never stop, remove or rename it,
  and never point workshop work at it.
- **Location**: `/home/jim/Documents/dbt-workshop` on XFS (moved off the exFAT drive `/run/media/jim/Jim`, which had no
  symlinks or permission bits). Symlinks, exec bits and venvs work; `.claude/skills/*` symlinks to `.agents/skills/*`.
  The workspace root is a git repo (base image binaries via submodules, see `.claude/skills/dbt-base-image`).
- **Throwaway resources for experiments**: SQL/dbt experiments go to a throwaway SingleStore (see §7), not the workshop
  stack and not the personal container.

## 1. Repository layout
```
/home/jim/Documents/dbt-workshop/
├── .agents/skills/            these skills (architecture, dbt-release-flow, dbt-reload-pipeline)
├── dbt-singlestore-workshop/  the dbt project + airflow/{dags,utils,tests} + .gitlab-ci.yml (pushed to GitLab)
├── infrastructure/            only folders + .gitignore + workshop.sh (keep it that way)
│   ├── workshop.sh            THE entry point (§2), incl. `session` for VS Code
│   ├── .gitignore             singlestore/olist_archive/, compose/.env
│   ├── compose/               docker-compose.yml (all services; relative paths are relative to compose/) + .env
│   │                          (GITLAB_CLIENT_ID/SECRET, written by bootstrap)
│   ├── manager/               manager image (Dockerfile, entrypoint.sh, requirements.txt - exact pins) and what it runs:
│   │                          bootstrap_stack.py (MANAGER_DIR, WORKSHOP_ROOT = infrastructure/, COMPOSE_DIR, COMPOSE_CMD,
│   │                          REPO_ROOT, DBT_PROJECT_DIR), reset_workshop.py (imports bootstrap_stack), milestones.sh
│   ├── airflow/               Airflow image (Dockerfile, entrypoint.sh, webserver_config.py) + mysql/init-mysql.sql
│   ├── gitlab/                gitlab.rb - Omnibus config of the upstream gitlab/gitlab-ce image (no image build)
│   ├── singlestore/           init_s2.sql, studio_schema.sql, studio_schema_dev.sql,
│   │                          olist_archive/*.csv (downloaded by bootstrap, git-ignored)
│   ├── workshop-studio/       studio backend (FastAPI), frontend (React Flow), scenarios/*.yaml, patches/
│   ├── dbt-singlestore-base-image/   Fusion dbt + SingleStore ADBC driver image (§5)
│   └── nginx/                 dbt-docs server config
├── README.md
```

## 2. `infrastructure/workshop.sh`
| Command | What it does |
| :-- | :-- |
| `up [svc...]` | `docker compose up -d --build` (the manager is not started: profile `manager`) |
| `bootstrap [--reset-only] [--initial-days N]` | `up`, then `bootstrap_stack.py` in the manager: SingleStore seed, GitLab PAT/OAuth app/groups/users/project/branches/tags, base image build + enterprise retags, runner registration, LDV+PRD build & deploy into the volumes (incl. docs), milestone `baseline`, Airflow + dbt-docs recreated. ~15 min on a fresh stack |
| `reset [args]` | `reset_workshop.py` in the manager. No args = full reset (re-seed, re-push project + baseline tags, redeploy, delete levers, fresh `baseline` milestone). `--trickle-dev N`, `--trickle-prod N [--enable-domain-b]`, `--channel <c> --env dev\|prod\|both --delay-batches/--duplicate-pct/...`, `--show-channels`, `--reset-channels`, `--milestone ...` |
| `milestone list \| create\|restore\|delete NAME` | `manager/milestones.sh` in the manager (§4) |
| `session [--devcontainer <folder>]` | VS Code lifecycle (see below): up now, stop when the last window / dev container closes |
| `shell` / `manager <cmd...>` | bash / any command in the manager |
| `stop`, `down`, `ps`, `logs`, `config`, `restart` | plain `docker compose` (`down -v` deletes ALL workshop volumes = a fresh stack) |

**Manager container** (`workshop-manager:latest`, service `manager`, profile `manager`, `restart: "no"`): python:3.13-slim
+ git + static Docker CLI 27.5.1 + compose plugin v2.32.4. `workshop.sh` runs
`docker compose --project-directory compose -f compose/docker-compose.yml --profile manager run --rm --build [-T] -v <repo>:<repo> -w <repo>/infrastructure/manager manager <cmd>`
(`-T` when there is no TTY). The repo is mounted at its **host** path because bootstrap issues `docker compose up` and
`docker run -v` whose bind sources the daemon resolves on the host. Inside, the stack is reached over `workshop_net`:
`S2_HOST=singlestore`, `S2_PORT=3306`, `GITLAB_URL=http://gitlab.local`, `GITLAB_CONTAINER`, `RUNNER_CONTAINER`,
`S2_CONTAINER`, `WORKSHOP_*` ports. Script defaults outside the manager: `127.0.0.1:23306`, `http://localhost`.
Bootstrap waits until SingleStore accepts root (a fresh container answers `1045 Access denied` until it has set
`ROOT_PASSWORD`) and until `gitlab-rails runner` works.

**Cluster lifecycle**: opening `dbt-singlestore-workshop/` in VS Code runs the folderOpen task (`.vscode/tasks.json`) ->
`infrastructure/workshop.sh session` (compose up, stop when the last window closes; one pid file per window in
`$XDG_RUNTIME_DIR/dbt-workshop-sessions`); the devcontainer's `initializeCommand` runs `workshop.sh session
--devcontainer <folder>` (a detached watcher stops the stack when the dev container stops). All services use `restart: on-failure`: the stack never starts at boot. Leave it stopped when done
(`workshop.sh stop`; the volumes keep the state).

## 3. Containers, ports, credentials (`infrastructure/compose/docker-compose.yml`)
Upstream images are pinned to verified versions (bump deliberately, then run a fresh bootstrap; SingleStore 0.2.90 =
engine 10.9.0, verified with a fresh bootstrap + E10 on 2026-11-08):
`ghcr.io/singlestore-labs/singlestoredb-dev:0.2.90`, `gitlab/gitlab-ce:19.5.3-ce.0`, `gitlab/gitlab-runner:alpine-v19.5.2`,
`mysql:8.4.13`, `nginx:1.31.8-alpine`, `alpine:3.22` - pulled from Docker Hub / ghcr.io, never built here. Built here:
`workshop-airflow`, `workshop-manager`, `workshop-studio` (compose) and `dbt-singlestore-base` (bootstrap).
| Container (in-network host) | Host port | Notes |
| :-- | :-- | :-- |
| `workshop-singlestore` (`singlestore`, `s2-dev`, `s2-prod`) | `23306` (`WORKSHOP_S2_PORT`), UI `8083` (`WORKSHOP_S2_UI_PORT`) | SingleStoreDB **Developer Edition**, `mem_limit 6g`. root / `WorkshopRootPwd123!`; `svc_ci_dbt`, `svc_ldv_dbt`, `svc_prd_dbt` / `CiServicePwd2026!`; personal `pu_*` and GitLab-named users / `WorkshopPwd2026!`. Data volume `workshop_s2_data` at **`/data`**. Log files 16 MB (`SINGLESTORE_SET_GLOBAL_LOG_FILE_SIZE_PARTITIONS/_REF_DBS`). No file mounts: Olist CSVs are streamed with `LOAD DATA LOCAL INFILE` (PyMySQL `local_infile=True`) |
| `workshop-gitlab` (`gitlab.local`) | `80`, ssh `2222` | GitLab CE (upstream image, no build), `mem_limit 4g`, config `infrastructure/gitlab/gitlab.rb` mounted as compose config `/omnibus_config.rb` + `GITLAB_OMNIBUS_CONFIG="from_file('/omnibus_config.rb')"` (re-applied on every start; state in volumes `gitlab_config`/`gitlab_data`/`gitlab_logs`). root / `WorkshopRootPwd123!`, PAT `glpat-workshop-root-token-2026`; group `workshop` (+ subgroups admins/maintainers/users/viewers), project `workshop/dbt-vdp`, users admin1, maintainer1-2, user1-2, viewer1-2 / `WorkshopPwd2026!`. CE: no CODEOWNERS enforcement |
| `workshop-gitlab-runner` | — | docker executor on `workshop_net`, image `artifacts.vdab.be/.../dbt-singlestore-base:1.11.11`; job containers get `--docker-volumes workshop_dev:/workshop_state/dev`, `workshop_prod:/workshop_state/prod` and the docker socket (registered by bootstrap; registration lives in the container, so a recreated runner needs `workshop.sh bootstrap`) |
| `workshop-mysql` (`mysql`) | — | `mysql:8.4.13` Airflow metadata (`airflow_dev`, `airflow_prod`; `airflow` / `airflow_db_password`; root `airflow_root_password`) |
| `workshop-airflow-dev` | `8088` (`WORKSHOP_AIRFLOW_LDV_PORT`) | LDV: `AIRFLOW_ENV_MODE=dev`, `IFW_PILLAR=rel1ldv`, `SINGLESTORE_USER=svc_ldv_dbt`, `WORKSHOP_STATE_VOLUME=workshop_dev` |
| `workshop-airflow-prod` | `8090` (`WORKSHOP_AIRFLOW_PRD_PORT`) | PRD: `prod`, `rel1prd`, `svc_prd_dbt`, `workshop_prod` |
| `workshop-dbt-docs` | `8081` (LDV), `8082` (PRD) | nginx on the `docs/` subpath of each workspace volume (read-only) |
| `workshop-studio` | `127.0.0.1:8085` only | presenter map; docker socket + GitLab PAT + S2 root - never expose it (§8) |
| `workshop-state-init` | — | one-shot `alpine:3.22`: creates `dags/utils dbt docs mnt_dbt/{target,artifacts}` in both workspace volumes; Airflow, dbt-docs, studio wait for it (`service_completed_successfully`) |
| manager (`dbt-workshop-manager-run-<id>`) | — | on demand only (§2) |

Airflow login: GitLab SSO (OAuth app "Airflow Workshop SSO"; redirect URIs use the LDV/PRD host ports, so after changing
`WORKSHOP_AIRFLOW_*_PORT` rerun `workshop.sh bootstrap` - `--reset-only` skips the GitLab rails step that updates them). GitLab subgroups map to
Airflow roles (`infrastructure/airflow/webserver_config.py`; users are `User` in dev, `Viewer` in prod).

## 4. Runtime state (Docker volumes only - no host folders)
1. **Workspaces `workshop_dev` / `workshop_prod`**: `dags/` (+ `dags/utils/`), `dbt/`, `docs/`, `mnt_dbt/{target,artifacts}`.
   Written by bootstrap/reset (`initialize_workshop_deployments`), the CI deploy job (`.deploy-airflow` ->
   `/workshop_state/<env>`; `LDV|ABT` -> dev volume, `CBT|PRD` -> prod volume) and the studio Reset
   (`redeploy_baseline`, GitLab archive of the baseline tag). Mounted by **subpath** (compose `volume: {subpath: ...}`):
   `dags` -> `/opt/airflow/dags` (DAGs `/opt/airflow/dags/*.py`, utils `/opt/airflow/dags/utils/`, `PYTHONPATH=/opt/airflow/dags`),
   `dbt` -> `/opt/airflow/dbt`, `mnt_dbt` -> `/mnt/dbt` (artifacts `daily_load`, `release/<run>`, `reload`, plus
   `target_freshness`, `target_drift_audit`).
   - dbt runner containers (`utils/dbt_pod_utils.py::_workspace_mounts`) are siblings via the host socket and mount by
     name: `--mount type=volume,src=$WORKSHOP_STATE_VOLUME,dst=/usr/app/dbt/workspace,volume-subpath=dbt` (+ `mnt_dbt`
     -> `/mnt/dbt`). Needs Docker CLI >= 26 / API >= 1.45: the Airflow image adds the static CLI 27.5.1 in
     `/usr/local/bin` (Debian's `docker.io` is 20.10) and compose sets `DOCKER_API_VERSION=1.47` (Docker 29 rejects
     < 1.44). `HOST_WORKSHOP_STATE_DIR` (bind mounts) is only a fallback.
   - Inspect: `docker run --rm -v workshop_prod:/w alpine:3.22 ls -R /w/dags`; `docker cp workshop-airflow-prod:/opt/airflow/dbt/<file> .`
2. **SingleStore milestones** (`manager/milestones.sh`, volume `workshop_s2_milestones`): named restore points of
   the WHOLE warehouse. The Developer Edition has no native ones - `CREATE MILESTONE` needs unlimited-storage databases
   (`ERROR 2633`), `BACKUP DATABASE` is Enterprise (`ERROR 1727`) - so a milestone is a tar copy of the **stopped**
   `workshop_s2_data` volume (helper `alpine:3.22`, copy to `<name>.tmp` then rename). Restores databases, tables,
   procedures, users, groups and grants; NOT Airflow metadata, GitLab or the workspace volumes. `baseline` (~2.5-3 GB) is
   taken at the end of bootstrap and full reset; restore incl. restart ~20 s. Without the 16 MB log files a seeded
   warehouse was 15.7 GB (preallocated 256 MB / 64 MB logs per partition) - the setting only applies to databases
   created on a fresh data volume. Studio: action `milestone`; Reset restores `baseline` first (fallback: re-seed
   channels + landing); API `GET /api/milestones`, `POST /api/milestones/<name>/create|restore|delete` (Live mode).
3. Other volumes: `workshop_studio_state` (studio `state.json`), `gitlab_*`, `mysql_data` (compose-prefixed
   `dbt-workshop_*`), runner cache volumes `runner-*`.

## 5. dbt engine & base image (`infrastructure/dbt-singlestore-base-image/`)
- `dbt-singlestore-base:1.11.11` = debian:trixie-slim + bash git curl sudo rsync + the Fusion `dbt` binary (`dbt-oss
  2.0.8`) + `libadbc_driver_singlestore.so` in `/usr/local/lib/dbt/` (dbt finds it walking up from its executable; it
  cannot reach SingleStore without it) + `DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true`. No Python, no dbt-core. git rewrites
  `git.vdab.be/vdp/` URLs to `http://gitlab.local/workshop/`. User `dbt` (uid 1000), workdir `/usr/app/dbt/workspace`.
- Bootstrap builds it from this folder and tags it as `dbt-singlestore-base:latest`, `artifacts.vdab.be/cops-docker-vdab/
  ondersteuning/dbt-singlestore-base:{1.11.11,1.9.2}`, `.../dbt-vdp/dbt-vdp:{ldv,abt,cbt,prd}`, `workshop-dbt-ci:latest`.
- Binaries: prebuilt `dbt` / `libadbc_driver_singlestore.so` in the folder (gitignored) are used as-is; when absent the
  Dockerfile builds them from the git submodules `src/dbt` (jimvekemans/dbt `feat/backfill-hardening-pr`) and
  `src/singlestore-adbc-connector` (`main`). Details, pitfalls (release build only, protobuf deps, memory,
  `-tags driverlib`): `.claude/skills/dbt-base-image/SKILL.md` and the folder's `README.md`.
- Fusion behaviour verified here: `--warn-error` does NOT promote warn-severity tests (severity decides); `env_var()`
  works in `+severity`, yml test configs, `config(enabled=...)` and per-version `enabled`; two same-named models are
  fine when exactly one is enabled; `ref(name, v=env_var(...))` resolves versions (`generate_alias_name` appends
  `_v<n>`, `parse_dbt_results` logs `<name>_v<n>`); adapter macros come from the binary (no project overrides of
  `singlestore__rename_relation` / `create_view_as`); `dbt docs generate --target run --output-dir <dir>` writes a full
  static site (`index.html`, `assets/`, `info_schema/`, ~13 MB) - there is no `--static`; profiles must not set
  `allow_cleartext_plugin: true` (refused without TLS; the workshop users use native passwords).
- `dbt --selector <name>` ignores CLI `--exclude` unless the exclusion is in `selectors.yml`; the `daily_load` selectors
  include `tag:critical` (source tests are otherwise never selected). An ignored file (`.dbtignore`) whose node is
  ref'd elsewhere fails the WHOLE parse (`dbt1048` / `dbt1062`).

## 6. SingleStore specifics
- **Databases**: landing `analytics_dev` / `analytics_prod` (+ `_olist_master` archive), model databases `edw_dv_rdv`,
  `edw_dv_bdv`, `im_dimensional`, management `vdp_mgmt` (run log `dbt_run_results`, reload state), CI `vdp_ci`,
  and `edw_ldm`, `edw_fmc`, `pres_dataproduct`, `tenant_ai` (no models build there; `perform_database_maintenance` /
  `clean_orphaned_models` still scan them). **LDV and PRD build the SAME model
  databases**; they differ in service account, landing database (`DBT_DEPLOY_TARGET` in `sources_olist.yml`) and
  Airflow. So staging of the generated feeds is `table`, never incremental (it would mix both environments' rows).
- **Two-part naming**: `node.database` is the SingleStore database, `node.schema` is `""` for service accounts and
  `pu_<user>` for personal users - resolve databases via `node.database` (fallback `target.database`), never `node.schema`.
- **RBAC**: user -> group -> role -> privileges (`init_s2.sql`; every `svc_*` is in `grp_admin` = `ALL ON *.*`).
  `information_schema.USERS_GROUPS`, `GROUPS_ROLES`, `ROLE_PRIVILEGES`, `USER_PRIVILEGES`, `SCHEMA_PRIVILEGES`; a
  least-privilege account sees only its own rows. No `IF [NOT] EXISTS` for roles/groups; error codes 1917 already in
  group, 1918 no such group, 1919 no such role, 1923 not in group; `DROP ROLE`/`DROP GROUP` cascade memberships.
  `information_schema.TABLES` has `CREATE_USER`, `ALTER_USER`, `ALTER_TIME` (`ALTER_USER` changes on ALTER TABLE, not on
  DML). Table materialization needs ALTER (rename) + DROP: `Error 1142: ALTER command denied ... for table`.
- **SQL quirks**: no multi-table `RENAME TABLE a TO b, c TO a` (ERROR 1064) - sequential `ALTER TABLE .. RENAME TO`;
  recursive CTEs with aggregation fail (ERROR 2730) - use cross-joined digit CTEs; `DATE` column vs
  `'YYYY-MM-DD HH:MM:SS'` truncates the literal to a date - cast to `DATETIME` for sub-day windows; `getopt: command not
  found` from the `singlestore` client wrapper inside the container is harmless.
- **Generated feeds** (`singlestore/studio_schema.sql`, built from `_olist_master` by `sp_build_generated_archives`):
  `carrier` -> `raw_carrier_tracking_events` (JSON payload; API v1 local time / v2 camelCase + UTC; `late_pct` holds
  non-final scans 2 batches in `_held_carrier_events`) and `web` -> `raw_web_events` (`customer_ref` v1 per-order id /
  v2 person; `bot_events` = one-cookie crawler; the loader detects a renamed `referrer`). Both trickle with every
  `sp_trickle_olist_batch(days, load_domain_b)`; each batch gets a strictly later `_ingested_at`. Per-channel faults in
  `_source_channel_config` / `_delivery_log` / `_held_batches`.
- **DEV-only** (`studio_schema_dev.sql`, `analytics_dev`): `_qa_test_accounts`, `sp_qa_smoke_test(n)` / `sp_qa_cleanup()`;
  source `qa` is enabled in LDV/ABT only; `stg_olist_orders` has a PRD and a DEV variant (`models/edw_dv_bdv/dev_only/`).
- dbt models of the feeds: `models/*/carrier/` (versioned `stg_carrier_tracking_events` v1/v2 via
  `DBT_VERSION_STG_CARRIER_TRACKING_EVENTS`, `fct_olist_deliveries`), `mart_olist_management_kpi`, `models/*/web/`,
  legacy `models/im_dimensional/legacy/int_web_top_pages`; tests in `tests/{carrier,web,olist}/`.

## 7. Airflow 3.3.2 (py3.14) patterns (`dbt-singlestore-workshop/airflow/`)
- Image `workshop-airflow:latest` (`infrastructure/airflow/Dockerfile`, `apache/airflow:slim-3.3.2-python3.14`): fab
  provider, `mysqlclient` (+gcc/pkg-config/libmariadb-dev), `aiomysql`, `pymysql`, conn `mysql+mysqldb://`;
  `AIRFLOW__CORE__EXECUTION_API_SERVER_URL=http://localhost:8080/execution/` (container-internal port; without it PRD
  tasks follow the published port and never run). All Airflow imports go through `utils/compat.py`; use
  `get_variable(key, default)`.
- dbt runs in sibling containers (`DBT_EXECUTION_MODE=docker`, `utils/dbt_pod_utils.py`) on `workshop_net`, `--user
  root`, `umask 0000` (artifacts must stay writable for the `airflow` uid 50000).
- Task subprocesses get `AIRFLOW__DATABASE__SQL_ALCHEMY_CONN=airflow-db-not-allowed:///`: `set_dag_paused_state`
  (`utils/dbt_dag_utils.py`) reads the real one from `/proc/1/environ` for `airflow dags pause/unpause`.
- `LocalExecutor` does not spawn a second worker for a waiting `TriggerDagRunOperator`: always `deferrable=True,
  poke_interval=5` with `wait_for_completion=True`.
- Run state comes from leaf tasks: never make an `all_done` task the only leaf; pair it with a
  `none_failed_min_one_success` leaf (`dbt_release`: `[cleanup_temporary_artifacts, unpause_daily_load]`).
- `dbt_daily_load` needs `manual_run_override=true` when triggered by hand; its result is decided by task `dbt_build`
  (the studio's `KEY_TASKS`). Tasks: `validate_trigger_source >> audit_environment_drift >> dbt_build >>
  persist_artifacts_to_state >> extract_relations_to_xcom >> trigger_cronacle_vdpreadysignal >> generate_dataset_outlets`, plus
  `dbt_source_freshness` (informational, own target path, `LOG_RUN_RESULTS=FALSE`).
- **Levers between deploy windows** (`utils/dbt_runtime_overrides.py`, read at task start): Variable `dbt_env_vars`
  (JSON env for every dbt task; secrets and `DBT_DEPLOY_TARGET` refused), `dbt_daily_load_dbtignore` (JSON list appended
  to `.dbtignore` for `dbt_build` + freshness, restored after), `disabled_dbt_models` (JSON list or comma/space string ->
  `--exclude`).
- Bootstrap `docker run` chains must not end in `|| true`; the landing procedures need `USE analytics_<env>` first.

## 8. Drift audit (OPS-01, studio scenario E10)
- `macros/ops/audit_environment_drift.sql` (`dbt run-operation audit_environment_drift`; args `fail_on_drift`
  (default true), `reference_user`, `audit_user`; vars `drift_audit_service_accounts`, `drift_audit_reference_user`,
  `drift_audit_ignore_databases` in `dbt_project.yml`). Findings: **changed outside dbt** (model relation whose
  CREATE/ALTER user is not a service account), **created outside dbt** (undeclared object in a model database, not by a
  service account or `pu_*`), **missing grant** (run user vs reference user: write privileges on model databases,
  SELECT on source databases).
- First task of `dbt_release` (gate; dry-run = report only) and report-only before `dbt_build` in `dbt_daily_load`
  (the build recreates the relation and erases the evidence). Own target path `/mnt/dbt/target_drift_audit`.
- As an admin (`svc_ci_dbt`): `--args '{audit_user: svc_prd_dbt}'` shows PRD's gap next to LDV's grants.
- Verified live 2026-11-07 on a fresh stack: daily load listed the root hotfix + hand-made view, the release gate failed
  on `svc_prd_dbt lacks DROP, ALTER on im_dimensional` before pausing anything, 0 findings after the fix.

## 9. Common operations
```bash
cd /home/jim/Documents/dbt-workshop
infrastructure/workshop.sh bootstrap                   # fresh stack (after `workshop.sh down -v`: everything from scratch)
infrastructure/workshop.sh up                          # start / apply compose + image changes
infrastructure/workshop.sh reset                       # full reset to the baseline
infrastructure/workshop.sh milestone create before-demo
infrastructure/workshop.sh milestone restore baseline  # SingleStore restarts (~20 s)
infrastructure/workshop.sh stop                        # leave it stopped when done
docker exec workshop-singlestore singlestore -p'WorkshopRootPwd123!' -e "SHOW DATABASES"   # SQL inside the stack
docker exec workshop-airflow-prod airflow dags trigger dbt_daily_load --conf '{"manual_run_override": true}'
```
Hot-fix the deployed code without CI (the regular path is commit -> GitLab -> deploy job, or the studio Reset):
```bash
for e in dev prod; do
  docker run --rm -v "$PWD/dbt-singlestore-workshop:/src:ro" -v "workshop_$e:/w" alpine:3.22 sh -c '
    apk add -q rsync >/dev/null
    rsync -rlD --delete --exclude=__pycache__ --exclude=utils /src/airflow/dags/ /w/dags/
    rsync -rlD --delete --exclude=__pycache__ /src/airflow/utils/ /w/dags/utils/
    rsync -rlD --exclude=.git --exclude=target --exclude=dbt_packages --exclude=logs /src/ /w/dbt/
    chmod -R a+rwX /w'
done
docker exec workshop-airflow-dev airflow dags reserialize; docker exec workshop-airflow-prod airflow dags reserialize
```
Studio code changes: `workshop.sh up` (rebuilds `workshop-studio`; scenarios, patches and `manager/milestones.sh` are mounted
and need no rebuild). Airflow image changes: `workshop.sh up` recreates both Airflow containers.

## 10. Tests
```bash
python3 -m venv ~/.venvs/dbt-workshop      # only for the offline test suites
~/.venvs/dbt-workshop/bin/pip install -r infrastructure/workshop-studio/backend/requirements-dev.txt
cd infrastructure/workshop-studio/backend && ~/.venvs/dbt-workshop/bin/python -m pytest -q      # studio, offline
cd dbt-singlestore-workshop/airflow && ~/.venvs/dbt-workshop/bin/python -m pytest -q tests     # Airflow utils
```
SQL suite against a throwaway SingleStore (never the workshop or personal instance):
```bash
docker run -d --name ms_s2 -p 127.0.0.1:33306:3306 -e ROOT_PASSWORD=TestPwd123 ghcr.io/singlestore-labs/singlestoredb-dev:0.2.90
STUDIO_TEST_S2_PORT=33306 STUDIO_TEST_S2_PWD=TestPwd123 ~/.venvs/dbt-workshop/bin/python -m pytest -q tests/test_sql_trickle.py
```
dbt experiments: put the throwaway on its own network, seed it with the manager image, run dbt from the base image
against a scratch copy of the project (dbt writes `target/` and `dbt_packages/` as root - clean up with a container):
```bash
docker network create ms_test && docker network connect ms_test ms_s2
docker run --rm --network ms_test -v "$PWD:$PWD" -w "$PWD/infrastructure/manager" -e S2_HOST=ms_s2 -e S2_PORT=3306 \
  -e S2_ROOT_PWD=TestPwd123 workshop-manager:latest python -c "import bootstrap_stack as b; b.seed_singlestore()"
docker run --rm --user root --network ms_test -v <scratch-copy>:/w -w /w -e SINGLESTORE_HOST=ms_s2 \
  -e SINGLESTORE_USER=svc_prd_dbt -e SINGLESTORE_PASS='CiServicePwd2026!' -e DBT_DEPLOY_TARGET=PRD \
  dbt-singlestore-base:1.11.11 bash -c "dbt deps && dbt build --target run"
```
Live studio checks: drive the API with `-H 'X-Studio: 1'` (`POST /api/mode {"mode":"live"}`, `POST
/api/scenarios/<id>/inject`, `POST /api/step/next`, `POST /api/reset`).
