---
name: vdab-dbt-workshop-architecture
description: >-
  Architecture and runbook of the self-contained dbt-singlestore workshop stack in this repository:
  repo layout, the root Makefile entry point and the manager container (bootstrap, reset), the workshop-* containers and
  ports, the three environments dev/val/prd (Airflow, state volume, docs site, prefixed SingleStore databases, roles and
  service user each), Docker volumes, the base, dev and dbt-vdp images, image-based deploys, Airflow 3.4.0 patterns, the overview page, the
  failure e-mail and Mailpit, the Vault stand-in, Airflow asset events, the drift audit, the tests and the Rust toolkit workspaces. Use it when starting,
  bootstrapping, resetting, debugging, testing or changing any part of the workshop stack.
---

# dbt-singlestore workshop: architecture & runbook

## 0. Ground rules
- **One entry point, no host Python for running the stack**: the root `Makefile` (`make help` lists the targets). The
  host needs Docker with the compose plugin and `make`; only `make test` also needs `python3` with `pytest`. Do not call
  the manager scripts directly. The Rust workspaces (§12) need no host toolchain either: `make rust-check` and friends
  run their quality gate in a container.
- **Self-contained**: containers are `workshop-<service>`, volumes `workshop_*`, network `workshop_net`, compose project
  `dbt-workshop`. The user's personal SingleStore (container `singlestore`, host 3306) belongs to other projects: never
  stop, change or target it.
- **No data ships with the stack**: the landing tables are created empty. The Workshop Studio
  (`infrastructure/workshop-studio/`, part of the default stack) is the only thing that fills them: a deterministic
  feeder seeds 90 days of a made-up web shop per environment, keeps today's rows arriving, and replays the last days
  under a demo's conditions. It also builds each environment once and deploys the baseline version to validation and
  production. Its scenarios, engine and clean-up model are described in `infrastructure/workshop-studio/README.md`.
- `infrastructure/dbt-singlestore-base-image/src/` holds git submodules (large); leave them out of searches.
  `.claude/skills/*` are symlinks to `.agents/skills/*`, except `dbt-base-image`, which lives in `.claude/skills/`
  itself. The Rust workspaces under `infrastructure/dbt-singlestore-dev-image/src/` have `target/` folders: leave them
  out of searches too.

## 1. Repository layout
```
dbt-workshop/
├── Makefile                   the entry point (§2)
├── requirements.md            the owner's own minimum requirements (the stack exceeds them: three environments)
├── .agents/skills/            these skills
├── .claude/                   skills/ (links to .agents/skills, plus dbt-base-image itself), launch.json (dev server
│                              `studio-frontend-mock`: Studio frontend on port 5199, `/?mock`)
├── dbt-singlestore-workshop/  the dbt project, Dockerfile, .gitlab-ci.yml, airflow/{dags,utils,tests}, .devcontainer/,
│                              .pre-commit-config.yaml, dbt-bouncer.yml, vdp.toml, tests/setup_tests.sh,
│                              scripts/docs_overview.sh (front page of the docs site, §6)
│                              (pushed to GitLab as workshop/dbt-vdp)
└── infrastructure/
    ├── compose/               docker-compose.yml (all services) + .env (GitLab OAuth credentials, written by bootstrap)
    ├── manager/               manager image + bootstrap_stack.py, reset_workshop.py
    ├── airflow/               Airflow image, webserver_config.py, mysql/init-mysql.sql (metadata databases)
    ├── gitlab/                gitlab.rb (Omnibus config of the upstream gitlab/gitlab-ce image)
    ├── singlestore/           init_global.sql, init_env.sql.tmpl
    ├── nginx/                 nginx.conf (docs sites)
    ├── vault/                 entrypoint.sh, provision.sh, README.md: the dev-mode Vault and its provisioning (§3, §10)
    ├── overview/              the overview page: app.py, index.html, services.json (§3)
    ├── dbt-singlestore-base-image/   Fusion dbt + ADBC driver image (§5)
    ├── dbt-singlestore-dev-image/    developer image (§5); src/ has sqlfmt, dbt-bouncer and the Rust workspaces
    │                                 vdp-rust-utils, release-ui, vaultspeed-framework (§12)
    ├── rust-check/            toolchain image (Dockerfile) and check.sh behind `make rust-check` / `rust-test` (§12)
    └── workshop-studio/       scenarios, live demos and the data feeder (§0; its own README)
```

## 2. `Makefile`
| Target | What it does |
| :-- | :-- |
| `up` | `docker compose up -d --build` (the manager has the profile `manager` and is not started) |
| `down` / `stop` / `ps` / `logs` | the plain compose commands; `S=<service>` limits them, e.g. `make logs S=airflow-dev` |
| `images` | builds `dbt-singlestore-base:1.11.11`, `dbt-singlestore-dev:1.11.11` and the Airflow and manager images |
| `bootstrap` | builds the base image on the host, then `up`, then `bootstrap_stack.py` in the manager |
| `reset` | `reset_workshop.py` in the manager, then `docker exec workshop-studio python -m studio.reset` (only a warning when the studio is not running or busy) |
| `shell` | bash in the manager |
| `test` | `pytest -q tests` in `dbt-singlestore-workshop/airflow` on the host, then `docker compose config -q` (also with the profile `manager`) |
| `rust-check` | the whole quality gate of the Rust workspaces in a container: fmt, clippy (`vdp-rust-utils`: also per cargo feature), tests, docs; `W=vdp-rust-utils\|release-ui\|vaultspeed-framework\|all` (default all) |
| `rust-test` | only the tests of `W`; `S=` goes to `cargo test`, e.g. `make rust-test W=vdp-rust-utils S="-p vdp-dbt"` |
| `rust-cargo` | any cargo command in the same container: `make rust-cargo W=release-ui S="fmt"` (`S` is required) |
| `rust-clean` | removes the toolchain image and the cache volumes of the three targets above |

**Bootstrap** (`infrastructure/manager/bootstrap_stack.py`), in order: apply `init_env.sql.tmpl` once per environment,
then `init_global.sql`; create the GitLab root token, the Airflow OAuth application and the group `workshop` (subgroups
`admins`, `maintainers`, `users`; root owns all three), the project `workshop/dbt-vdp`, one maintainer and the users `pu_dev1..3`; check that the base
image exists (it must: the Makefile builds it, the manager's docker CLI has no BuildKit) and note when the dev image is
missing (`make images` builds it); register the runner; recreate the three Airflows and the docs server; push
the dbt project (branches `dev` and `main`, tag `v1.0.0`) and wait for the pipeline of that tag, which builds
`dbt-vdp:v1.0.0` and deploys it to dev. Bootstrap ends by resetting the Workshop Studio (`studio.reset --wait`) and
waiting for its baseline: the studio starts with `up`, before the tables and the image exist, so its first check fails
(a failure here is only a warning with the manual command).
**Reset** (`reset_workshop.py`): drop and re-create the environment databases and `vdp_ci`, reset the GitLab repository
to the baseline (which starts the first pipeline again), delete the Airflow runtime-override Variables
(`disabled_dbt_models`, `dbt_daily_load_dbtignore`, `dbt_env_vars`, `dbt_dataset_dependencies`, `dbt_emit_asset_events`).

**Manager container** (`workshop-manager:latest`, service `manager`, profile `manager`): Python plus the Docker CLI and
compose. The Makefile runs it with the repo mounted at its **host** path, because bootstrap issues `docker compose up` and
`docker run -v` whose bind sources the daemon resolves on the host. Inside it the stack is reached over `workshop_net`
(`S2_HOST=singlestore`, `GITLAB_URL=http://gitlab.local`).

## 3. Containers, ports, credentials (`infrastructure/compose/docker-compose.yml`)
Upstream images are pinned to exact versions (bump deliberately, then bootstrap on a fresh stack). Built here: the Airflow
image, the manager, the overview, the studio, and the dbt images.

| Container (in-network host) | Host port | Notes |
| :-- | :-- | :-- |
| `workshop-singlestore` (`singlestore`) | `23306` (`WORKSHOP_S2_PORT`), UI `8083` (`WORKSHOP_S2_UI_PORT`) | SingleStoreDB Developer Edition, `mem_limit 6g`, data in `workshop_s2_data` at `/data`, 16 MB log files |
| `workshop-gitlab` (`gitlab.local`) | `80`, ssh `2222` | GitLab CE, `mem_limit 4g`, config `infrastructure/gitlab/gitlab.rb` |
| `workshop-gitlab-runner` | none | docker executor on `workshop_net`, default job image `dbt-singlestore-base:1.11.11`, docker socket mounted; registered by bootstrap (a recreated runner needs `make bootstrap`) |
| `workshop-mysql` (`mysql`) | none | Airflow metadata: `airflow_dev`, `airflow_val`, `airflow_prd` |
| `workshop-airflow-dev` / `-val` / `-prd` | `8088` / `8089` / `8090` (`WORKSHOP_AIRFLOW_DEV_PORT`, `_VAL_`, `_PRD_`) | one per environment; env `DBT_DEPLOY_TARGET`, `WORKSHOP_STATE_VOLUME`, `SINGLESTORE_USER` |
| `workshop-dbt-docs` | `8081` dev, `8082` val, `8086` prd | one nginx, one server block per environment (`infrastructure/nginx/nginx.conf`), reads `docs/` of each state volume |
| `workshop-mailpit` | UI `8025` (`WORKSHOP_MAILPIT_PORT`) | SMTP `mailpit:1025`, no auth or TLS (§7) |
| `workshop-vault` (`vault`) | `127.0.0.1:8200` (`WORKSHOP_VAULT_PORT`) | HashiCorp Vault 1.20.4 in dev mode (in memory, plain HTTP, `mem_limit 256m`), UI `/ui`. Holds the developers' SingleStore password in KV v2 mount `kv-workshop`: `vdp/<env>/dbt` (key `SINGLESTORE_PASS`, read by `vdp`) and `users/pu_dev1..3`; one read-only AppRole per developer. `infrastructure/vault/entrypoint.sh` provisions it on every start (dev mode forgets everything), healthy when done. Workshop credentials: root token `workshop-root-token`; AppRole `pu_devN`: role id `workshop-role-id-pu_devN`, secret id `workshop-secret-id-pu_devN`. Only developers who choose the vault way and the vault stage of the Studio flow `developers-toolbox` (`toolbox.run` with `auth: vault`) use it; the rest of the stack runs with it stopped (§10) |
| `workshop-overview` | `127.0.0.1:8099` (`WORKSHOP_OVERVIEW_PORT`) | overview page (`infrastructure/overview/`): container state from the docker socket (read-only), health probes inside `workshop_net`, SingleStore users per environment and GitLab users read live, links to the web apps; each user's password is hidden until shown (per row or all at once) and can be copied, and is read from where the stack defines it (container environments, `infrastructure/singlestore/*.sql*`, `SHARED_USER_PWD` in `bootstrap_stack.py`, the last two mounted read-only); the service list is `services.json`, which mirrors the Airflow role mapping of `webserver_config.py` |
| `workshop-state-init` | none | one-shot: creates `dags/utils`, `docs`, `mnt_dbt/target`, `mnt_dbt/artifacts` in the three state volumes; Airflow, docs and studio wait for it |
| `workshop-studio` | `127.0.0.1:8085` (`WORKSHOP_STUDIO_PORT`) | scenarios and live demos (§0); drives GitLab, Airflow and SingleStore and starts dbt containers, so it has the docker socket and the GitLab and SingleStore root credentials: never expose it |
| manager | none | on demand (§2) |

**Credentials** are fixtures, defined in: `infrastructure/singlestore/init_global.sql` and `init_env.sql.tmpl`
(SingleStore users), `infrastructure/compose/docker-compose.yml` (SingleStore root, Airflow service passwords, MySQL),
`infrastructure/manager/bootstrap_stack.py` (GitLab root, token and users). The dbt profiles
(`dbt-singlestore-workshop/profiles.yml`) take the connection from `SINGLESTORE_HOST/USER/PASS`.

**Airflow login** is GitLab SSO (OAuth application "Airflow Workshop SSO", created or updated by
`bootstrap_gitlab_rails`). The redirect URIs are `http://{localhost,127.0.0.1}:<dev|val|prd port>/auth/oauth-authorized/gitlab`:
Airflow 3 serves FAB under `/auth`, and GitLab requires an exact match, otherwise it shows "The redirect URI included is
not valid". The URIs use the host ports, so after changing `WORKSHOP_AIRFLOW_*_PORT` rerun `make bootstrap`. That step
renews the application secret on every run, because GitLab stores it hashed and an existing one cannot be read back, and
writes it to `infrastructure/compose/.env`. GitLab subgroups map to Airflow roles in
`infrastructure/airflow/webserver_config.py`: `workshop/admins` is `Admin` (root), `workshop/maintainers` is `Op`,
`workshop/users` is `User` in dev and `Viewer` in val and prd. After the GitLab sign-in the Airflow container itself
calls `http://gitlab.local/oauth/token`: that name must resolve to the GitLab container (its alias on
`workshop_net`). Never add an `extra_hosts` entry that sends it through the host: host firewalls drop that route,
the callback hangs for two minutes and the sign-in fails ("Error authorizing OAuth access token ... timed out" in
the Airflow log).

The three Airflows share two fixed, workshop-only keys in `docker-compose.yml`: `AIRFLOW__CORE__FERNET_KEY` (encrypts
Variables such as `dbt_image_tag`) and `AIRFLOW__API_AUTH__JWT_SECRET` (signs sign-in sessions). Without them every
container invents its own keys when it is created, so after any recreate the stored Variables read as "Variable
dbt_image_tag does not exist" (no DAG can find its image, the studio's baseline check fails) and everybody is signed
out. Changing either key has the same effect once: set the Variables again (`airflow variables set dbt_image_tag
<tag>`, or `docker exec workshop-studio python -m studio.reset`, which redeploys the baseline version).

## 4. Environments, state and volumes
Three environments, `dev`, `val`, `prd`, selected by the env var `DBT_DEPLOY_TARGET` (default `dev`). Each has its own
Airflow, state volume (`workshop_dev`, `workshop_val`, `workshop_prd`), docs site, databases and service user.

| | dev | val | prd |
| :-- | :-- | :-- | :-- |
| Fed by | tag `vX.Y.Z` on `dev` | tag `YYYY-Qx.N` on `main` | same tag, manual job |
| Service user | `svc_dev_dbt` | `svc_val_dbt` | `svc_prd_dbt` |

State volume layout: `dags/` (with `dags/utils/`), `docs/`, `mnt_dbt/{target,artifacts}`. Airflow mounts the subpaths
`dags` at `/opt/airflow/dags` and `mnt_dbt` at `/mnt/dbt`; `PYTHONPATH=/opt/airflow/dags`. The dbt project is not in the
volume: it is in the image (§6). Inspect: `docker run --rm -v workshop_val:/w alpine:3.22 ls -R /w/dags`.

Other volumes: `workshop_s2_data` (SingleStore), `gitlab_config`/`gitlab_data`/`gitlab_logs` and `mysql_data`
(compose-prefixed `dbt-workshop_*`), and `workshop_studio_state` (the studio's state: what a demo left behind, feeder state). `make down`
keeps the volumes; `docker compose down -v` (run in `infrastructure/compose`) deletes them all.

## 5. Images
- **Base** `dbt-singlestore-base:1.11.11` (`infrastructure/dbt-singlestore-base-image/`): Debian, Fusion `dbt` in
  `/usr/local/bin`, `libadbc_driver_singlestore.so` in `/usr/local/lib/dbt/` (dbt finds it walking up from its
  executable), `DBT_ALLOW_EXPERIMENTAL_ADAPTERS=true`, user `dbt` (uid 1000), workdir `/usr/app/dbt/workspace`. No Python,
  no dbt-core. Prebuilt binaries in that folder are used as is, otherwise it builds from the submodules. Details:
  `.claude/skills/dbt-base-image/SKILL.md`.
- **Dev** `dbt-singlestore-dev:1.11.11` (`infrastructure/dbt-singlestore-dev-image/`): base plus `vdp` (`dbt` on the PATH
  is `vdp`, which runs the real binary from `/usr/local/libexec/dbt/dbt`), the `sqlfmt` and `dbt-bouncer` forks and
  `pre-commit` in `/opt/devtools`. Everything is built from the image folder, like the base image: `vdp` is the
  prebuilt `./vdp` when present, else a release build of `src/vdp-rust-utils`; `src/sqlfmt` and `src/dbt-bouncer` are
  pip-installed. `src/` (sqlfmt, dbt-bouncer, vdp-rust-utils, vaultspeed-framework, release-ui) is tracked and the vdp
  source is a cleaned copy; only `local/` (the untouched client copies) and `./vdp` are gitignored. See its `README.md`
  and §12.
- **Project** `dbt-vdp:<git tag>`: built by CI from `dbt-singlestore-workshop/Dockerfile` (`FROM
  dbt-singlestore-base:1.11.11`, project copied to `/usr/app/dbt/workspace` including `airflow/dags` and
  `airflow/utils`, `dbt deps` run at build). There is no registry: runner and Airflow use the host Docker daemon, so the
  local image store is the registry.
- **Airflow** `workshop-airflow:latest` (`infrastructure/airflow/Dockerfile`, `apache/airflow:slim-3.4.0-python3.14`). The 3.4 images are Docker
  Hardened Images whose system packages only install from an apt source that needs a Docker login, so nothing is
  installed with apt: the `mysqlclient` wheel is built in a `python:3.14-slim-trixie` stage and copied in.

## 6. Deploys (`dbt-singlestore-workshop/.gitlab-ci.yml`)
A tag builds `dbt-vdp:<tag>` (`build_image`); `publish_artifacts` (not fatal) stores the version's catalogue artifacts
(`dbt compile --target collibra`) and documentation as generic package `dbt-artifacts` / `<tag>` in GitLab; a deploy job
then, for environment E: (1) copies `airflow/dags` and `airflow/utils` out of the image into `workshop_E` (`dags/`,
`dags/utils/`, stale files deleted); (2) writes the front page of the docs site (`scripts/docs_overview.sh`: version,
environment, time, counts, teams; best effort) and runs `dbt docs generate --target run --output-dir /state/docs` from the
image as `svc_E_dbt`; (3) runs `docker exec workshop-airflow-E airflow variables set dbt_image_tag <tag>`; (4) stores the
environment's copy of the artifacts (generic package `dbt-artifacts-env` / `E`, with `deployed.json`; not fatal). Airflow runs every dbt task as
`docker run --rm --network workshop_net` of `dbt-vdp:<Variable dbt_image_tag>` with only the `mnt_dbt` subpath of its
state volume mounted (`airflow/utils/dbt_pod_utils.py`, `dbt_config.py`). If the Variable is unset the task fails with a
message saying so. **Rollback** = set the Variable to an older tag (the DAG files stay those of the last deploy).
Release flow and job list (its §2 repeats this deploy): `.agents/skills/dbt-release-flow/SKILL.md`.

## 7. Airflow patterns (`dbt-singlestore-workshop/airflow/`)
- DAGs: `dbt_daily_load` (plain schedule from `DBT_DAILY_LOAD_SCHEDULE`, default `@daily`, `none` = manual; trigger by
  hand with `docker exec workshop-airflow-<env> airflow dags trigger dbt_daily_load`), `dbt_release` (§8), `dbt_reload`
  (`.agents/skills/dbt-reload-pipeline/SKILL.md`), `dbt_housekeeping` (weekly, only created in dev:
  `cleanup_user_models`, `drop_old_relations`), `dbt_on_demand` (no schedule, created paused; params `select` (default
  `tag:on-demand`; never empty, and a selection that matches nothing fails), `exclude`, `full_refresh_models`; own target
  and state folders `target_on_demand` and `artifacts/on_demand`; `dbt build ... --target run`, `dbt retry` for a cleared
  task) and `report_refreshed_notice` (created paused; scheduled on the asset of
  `<env>_im_dimensional.mart_olist_management_kpi`; one plain-text notice mail to Mailpit per run, a test notice when
  started by hand).
- `dbt_daily_load` tasks: `audit_environment_drift` (report only), `dbt_build`, `persist_artifacts_to_state`,
  `dbt_source_freshness` (informational, own target path), then `send_failure_report`.
- **Assets** (`utils/dbt_asset_utils.py`; both Variables are off until set and are read when the DAG file is PARSED, about
  every 30 s, not when a task starts). An asset URI is `s2://workshop-<env>/<database>/<object>`. `dbt_dataset_dependencies`
  (JSON list of URIs): non-empty, `dbt_daily_load` has no clock schedule and starts when every listed asset has a new
  event (the Studio reports the feeds' landing tables this way); empty or unset, `DBT_DAILY_LOAD_SCHEDULE` applies.
  `dbt_emit_asset_events` (`true`): the tasks `extract_refreshed_tables` (models with status success in `run_results.json`,
  minus models that failed a test of their own; skipped when the file is missing or older than the run) and
  `emit_asset_events` (one event per table through the alias `dbt_refreshed_tables`, `extra` with status, counted rows,
  build time and `dbt_image_tag`) run after `persist_artifacts_to_state`; without the Variable they do not exist.
  `make reset` deletes both Variables.
- Airflow imports come from `airflow.sdk`; `get_variable(key, default)` is in `utils/dbt_pod_utils.py`.
- `utils/dbt_pod_utils.py` has an abstract `DbtTaskRunner` with two implementations, chosen by `DBT_RUNTIME`
  (`dbt_config.py`): `DockerTaskRunner` (default, this stack) and `OpenShiftTaskRunner` (a pod per task through
  `@task.kubernetes_cmd`; needs `apache-airflow-providers-cncf-kubernetes`, which the workshop Airflow image does not
  ship; namespace, secret, PVC and image come from `DBT_OPENSHIFT_*`, defaults are VDAB's). The DAGs only use
  `dbt_task`, `python_task` and `state_call`. A `python_task` callable returns
  `state_call(<function of dbt_state_utils>, **json_kwargs)` or None: Docker makes the call in the Airflow container,
  OpenShift ships it to a pod that needs python (`DBT_OPENSHIFT_STATE_IMAGE`; the workshop's dbt image has none).
  `python_task(do_xcom_push=True)` makes the function's result the XCom, `skip_if_missing=True` turns its
  FileNotFoundError into a skipped task. The same `dbt_pod_utils.py` and `tests/test_pod_utils.py` are used by the
  VDAB `dbt-vdp` project (no Airflow 2 fallback imports remain); project differences live in
  `dbt_config.py`, which derives `DEPLOY_TARGET` and `SINGLESTORE_DB` (`<env>_edw_dv_rdv`) for the dbt containers, falls
  back to the platform's `IFW_PILLAR` when `DBT_DEPLOY_TARGET` is unset and holds the OpenShift settings, among them
  `DBT_OPENSHIFT_KUBE_CONFIG` (empty = in-cluster) and `OPENSHIFT_OPTIONAL_SECRET_KEYS` (secret keys a pod may start
  without: `IKL_HASH_SALT`).
- dbt runs in sibling containers started through the docker socket, `--user root`, `umask 0000` (artifacts stay writable
  for the `airflow` user). The Airflow image ships a docker CLI 29.x (volume subpaths need CLI >= 26) and
  compose sets `DOCKER_API_VERSION=1.47`.
- `AIRFLOW__CORE__EXECUTION_API_SERVER_URL=http://localhost:8080/execution/` (container-internal port); without it tasks
  follow the published host port and never run.
- Task subprocesses get no metadata DB connection: `set_dag_paused_state` (`utils/dbt_dag_utils.py`) reads the real one
  from `/proc/1/environ` for `airflow dags pause/unpause`.
- A waiting `TriggerDagRunOperator` needs `deferrable=True, poke_interval=5`, as `dbt_release` has it for `dbt_reload`.
- Run state comes from leaf tasks: never make an `all_done` task the only leaf (`dbt_release` pairs
  `cleanup_temporary_artifacts` with `unpause_daily_load`).
- **Runtime levers** (`utils/dbt_runtime_overrides.py`, read when a task starts): Variable `dbt_env_vars` (JSON env for
  every dbt task; `SINGLESTORE_*`, `DBT_DEPLOY_TARGET`, `IKL_HASH_SALT`, `DBT_PROFILES_DIR`, `DBT_TARGET_PATH`, `PATH` and
  `HOME` are refused), `dbt_daily_load_dbtignore` (JSON list appended to
  `.dbtignore` for the build and the freshness task), `disabled_dbt_models` (appended to `--exclude`).
- **Failure e-mail** (`utils/dbt_failure_report.py`, task `send_failure_report` with `trigger_rule="one_failed"` in
  `dbt_daily_load` and `dbt_release`): one HTML mail per DAG run with the errors and warnings parsed from the task logs,
  `run_results.json` when it exists, and the log tail. The Airflow containers send it to `mailpit:1025`
  (`DBT_REPORT_SMTP_HOST`, `DBT_REPORT_SMTP_PORT`, `DBT_REPORT_MAIL_FROM=airflow-<env>@workshop.local`,
  `DBT_FAILURE_EMAIL_RECIPIENTS`); read the mails at http://localhost:8025. The patterns target real dbt 2.0.11 output (fixtures in `airflow/tests/fixtures`); only the failed-MODEL line is still inferred from the failed-test line. The mail is written in spoken Flemish (style guide: `.agents/skills/workshop-studio-scenarios/references/vlaams.md`); quoted dbt and Airflow text stays as it is. Subject: `[development] Nachtelijke load mislukt: 1 check mislukt, 6 stappen niet gerund` (the studio finds the mail by `Nachtelijke load mislukt`). The notice mail of `report_refreshed_notice` has the subject `[development] Het managementrapport is ververst` (a hand-started run sends `[development] Testverwittiging: manueel gestart`); the studio finds it by `Het managementrapport is ververst`.

## 8. `dbt_release`, drift audit
- Task order: `audit_environment_drift` (gate) -> `pause_daily_load` -> `run_operation__database_maintenance` -> the steps
  of `RELEASE_STEPS` (with state merging, reload delegation) -> `run_operation__sync_grants`
  (`sync_grants_with_singlestore`) -> `run_operation__clean_orphaned_models` (`clean_orphaned_models`) ->
  `cleanup_temporary_artifacts` and `unpause_daily_load`; `send_failure_report` after all dbt tasks. Details:
  `.agents/skills/dbt-release-flow/SKILL.md`.
- **Drift audit** (`macros/ops/audit_environment_drift.sql`, `dbt run-operation audit_environment_drift`; args
  `fail_on_drift` (default true), `reference_user`, `audit_user`; vars `drift_audit_service_accounts`,
  `drift_audit_reference_user`, `drift_audit_ignore_databases` in `dbt_project.yml`). Findings: a model relation
  changed outside dbt (CREATE/ALTER user is not a service account), an object created outside dbt, a missing grant (run
  user vs reference user). It is the first task of `dbt_release` and report-only before `dbt_build` in `dbt_daily_load`
  (the build recreates the relation and erases the evidence); own target path `/mnt/dbt/target_drift_audit`.

## 9. SingleStore layout, RBAC, quirks (`infrastructure/singlestore/`)
- **One instance, separate databases per environment E** (applied from `init_env.sql.tmpl`, `__ENV__` is replaced):
  `E_landing` (empty landing tables), `E_edw_dv_rdv`, `E_edw_dv_bdv`, `E_im_dimensional`, `E_vdp_mgmt` (run log
  `dbt_run_results`, `vdp_parameters`; the reload macros create `reload_state` there on demand). Global (`init_global.sql`): `vdp_ci`, the CI
  scratch database.
- **Roles** (user -> group `grp_<role>` -> role -> privileges, grants written per database): per environment `E_admin`
  (ALL), `E_dbt` (ALL on the four model/management databases, SELECT on `E_landing`) and `E_reader` (SELECT); globally
  `ci_dbt` (ALL on `vdp_ci`) and `dev_developer` (like `dev_dbt`).
- **Users**: `svc_dev_dbt`, `svc_val_dbt`, `svc_prd_dbt` (CI deploy and Airflow, role `E_dbt` only), `svc_ci_dbt` (`ci_dbt`
  and `dev_reader`), developers `pu_dev1..3` (`dev_developer`, `val_reader`, `prd_reader`), admin `pu_admin` (the three
  `E_admin`).
- **dbt naming**: profile targets `default` (developer), `ci`, `run`, `reload`, `collibra`. Database of
  `default`/`run`/`reload`/`collibra` is `<DBT_DEPLOY_TARGET>_edw_dv_rdv`, of `ci` is `vdp_ci` (`profiles.yml`). `collibra` is
  only for `dbt compile --target collibra --target-path target/collibra` (the catalogue artifacts of `publish_artifacts`):
  `generate_schema_name` returns the model's database and `generate_alias_name` the plain model name, so the catalogue lists
  the real `<env>_edw_dv_bdv`.`<model>` without user or branch prefix. `generate_database_name`
  (`macros/core/generate_database_name.sql`): target `ci` always gives `vdp_ci`; otherwise a custom database `X` gives
  `<env>_X`, and no custom database gives `target.database`. Developer runs use the user name as schema, so
  `generate_alias_name` yields `<user>_<model>` inside the `dev_*` databases (never resolve a database through
  `node.schema`). Sources: landing is `<env>_landing`, management `<env>_vdp_mgmt`.
- Information schema: `USERS_GROUPS`, `GROUPS_ROLES`, `ROLE_PRIVILEGES`, `USER_PRIVILEGES`, `SCHEMA_PRIVILEGES`;
  `TABLES.CREATE_USER` / `ALTER_USER` are what the drift audit reads. Role and group creation has no `IF NOT EXISTS`.
- SQL quirk used in code: no multi-table `RENAME TABLE`, so the blue/green swap renames sequentially (the table is missing for a fraction of a second, about 0.1 s measured)
  (`macros/utils/reload_helpers.sql`).
- dbt Fusion notes tied to code: `--warn-error` does not promote warn-severity tests, the severity decides
  (`+severity` reads `DBT_TEST_SEVERITY` in `dbt_project.yml`); the carrier scan model `stg_carrier_events` reads the old
  and the new payload format split at `DBT_CARRIER_V2_FROM` (Airflow Variable `dbt_env_vars`); adapter macros come from the binary; a `.dbtignore` entry whose node is still ref'd fails the
  whole parse; `profiles.yml` must not set `allow_cleartext_plugin` (refused without TLS).
- dbt project: `import_dependencies` and `star` (`macros/utils/`) are used in `stg_olist_customers` and
  `mart_olist_management_kpi`.
- **Guards in the macros** (documented in `macros/core/core.yml` and `macros/utils/utils.yml`): `should_execute()` (real
  build, run, run-operation or snapshot, not `--empty`) wraps every hook that writes; `should_log_results()` (the same, plus
  a real `test` or `seed`) decides whether the on-run-end hook writes the run logbook; `should_full_refresh()` overrides
  dbt's built-in: `--vars '{full_refresh_models: [...]}'` refreshes exactly the listed models, and
  `meta.full_refresh_only_when_listed` protects a table from a bare `--full-refresh`. The model `dummy_load_cycles` is the
  loading history that `open_load_cycle` / `close_load_cycle` fill around `dummy_reload_pit_with_deps`. The report
  `mart_olist_customer_cohorts` (`models/im_dimensional/on_demand/`, tag `on-demand`) is left out of the nightly load and
  is built by `dbt_on_demand` (§7).

## 10. Developer side
`dbt-singlestore-workshop/.devcontainer/` (image `dbt-singlestore-dev:1.11.11`, `--network=workshop_net`; its
`initializeCommand` runs `make up` on the host). `post_create.sh` asks for a `pu_dev*` account and either its password or the vault way (below) and writes
`.devcontainer/.env` (gitignored; template `.env.example`), runs `dbt deps` and `pre-commit install`.
`.pre-commit-config.yaml` runs `sqlfmt`, `dbt parse` and `dbt-bouncer` (`dbt-bouncer.yml`). `vdp.toml` defines the
environments dev, val and prd for `vdp` (Vault at `http://vault:8200` and no CA certificate; they are aliases of vdp's built-in LDV, ABT and
PRD, so `{env}` renders `ldv`/`abt`/`prd` and `{env_short}` renders `dev`/`val`/`prd`, and `[qa] run_results_table =
"{env_short}_vdp_mgmt.dbt_run_results"` points the qa commands at the run log of §9), the `dev_` databases its source commands
work on, and `[sources.database_prefix]`: `vdp` writes `{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_<name>` instead of
the literal `dev_<name>`, so generated sources (`models/sources/sources_<name>.yml`) are valid in every
environment and the hand-written sources that use the expression can be extended.
`tests/setup_tests.sh` builds only the models modified against the merge-base with `origin/dev`, in one of three schema
modes (user prefix, dev prefix, branch schema). VS Code tasks in `.vscode/tasks.json` do the same steps, and the
`vdp:` tasks wrap the `vdp` commands that work in the workshop: `env show`, `env doctor`, `sources add` (declare a
table, or `--missing` for every undefined source), `sources rewrite` (it also deletes a generated file whose tables
nothing references), `sql run`, `sql export`, `qa orphan-tables`, `qa view-candidates`. In the devcontainer `dbt` is
`vdp`, which lets `.devcontainer/.env` override the process environment.

**Password source** (`[secrets].precedence = ["vault:vdp/{env_short}/dbt", "dotenv", "env"]`, mount `kv-workshop`): Vault wins, but only when `KEYVAULT_ROLE_ID` and `KEYVAULT_SECRET_ID` are set (`.devcontainer/.env` or the process environment); without them Vault is skipped quietly, so a `.env` with `SINGLESTORE_PASS` and every CI, Airflow and Studio run behave as before and a stopped vault changes nothing. `post_create.sh` offers the vault way (default: no); `.env` then holds host, user, `DBT_DEPLOY_TARGET` and the two ids. `vdp` reads one path per environment and has no user placeholder, so the shared path holds the password all `pu_dev*` accounts have; personal records `users/pu_dev*` are for a future `{user}` placeholder. Check with `vdp env show` (shows the layer, never the value). See `infrastructure/vault/README.md`.


## 11. Tests and experiments
- `make test` (host `pytest` for `airflow/tests`: `test_runtime_overrides.py`, `test_failure_report.py`, `test_pod_utils.py`, `test_release_steps.py`, `test_asset_utils.py`, `test_dag_wiring.py`; compose config check).
- SQL or dbt experiments go to a throwaway SingleStore, never the workshop stack or the personal container:
  ```bash
  docker run -d --name ms_s2 -p 127.0.0.1:33306:3306 -e ROOT_PASSWORD=<pick one> ghcr.io/singlestore-labs/singlestoredb-dev:0.2.90
  ```
  Create the databases and roles by applying `init_env.sql.tmpl` (replace `__ENV__`) and `init_global.sql` to it, put it on its
  own docker network with a dbt container from `dbt-singlestore-base:1.11.11`, and run dbt with `SINGLESTORE_HOST` set to it.

## 12. Rust toolkit workspaces (`infrastructure/dbt-singlestore-dev-image/src/`)
Three tracked Cargo workspaces (Rust 1.96), each with its own skill. Only `vdp` is built into an image.

| Workspace | Binary (crate to build) | Built where | Consumers | Skill |
| :-- | :-- | :-- | :-- | :-- |
| `vdp-rust-utils` | `vdp` (`cargo build --release --locked -p vdp-cli`) | dev image, stage `vdp-build` (`rust:1.96-trixie`); a prebuilt `./vdp` wins; with neither the image has plain `dbt` | `.devcontainer` + `vdp.toml`, the `vdp:` tasks in `.vscode/tasks.json`, pre-commit (`dbt parse`) and `tests/setup_tests.sh` (both call `dbt`, the link), Workshop Studio | `src/vdp-rust-utils/.agents/skills/vdp-rust-utils/SKILL.md` |
| `release-ui` | `vdp-release-ui` (`-p vdp-release-ui-cli`) | by hand, in no image | none in the stack | `src/release-ui/.agents/skills/vdp-release-ui/SKILL.md` |
| `vaultspeed-framework` | `vdp-vaultspeed` (`-p vdp-vaultspeed-cli`) | by hand, in no image | none in the stack | `src/vaultspeed-framework/.agents/skills/vdp-vaultspeed-framework/SKILL.md` |

- In the dev image `dbt` on the PATH is a link to `vdp`; the real engine is `/usr/local/libexec/dbt/dbt` (`VDP_DBT_BIN`).
  CI, Airflow and the project image run the plain `dbt` of the base image.
- `vdp vaultspeed ...` and `vdp release-ui ...` replace `vdp` by `vdp-vaultspeed` / `vdp-release-ui`, found next to `vdp`
  or on `PATH`; without the binary `vdp` says so and exits 2. Neither workspace is referenced by compose, the Makefile,
  nginx, the overview, CI or `vdp.toml`.
- Path dependencies run `vdp-rust-utils` <- `release-ui` <- `vaultspeed-framework`: keep the three folders side by side.
- **Checks run in Docker, not on the host.** `make rust-check` (fmt, clippy, tests, docs; `vdp-rust-utils` also clippy per
  cargo feature), `make rust-test` (`S=` goes to `cargo test`) and `make rust-cargo S="<cargo arguments>"` run in the image
  `workshop-rust-check:<version>` (`infrastructure/rust-check/`: `rust:<version>-trixie` plus rustfmt and clippy under the
  channel name the workspaces pin). `W=<workspace>` picks one workspace (default all three, in dependency order). The
  version is read from `ARG RUST_VERSION` of the dev image Dockerfile, so a toolchain bump touches that line, the three
  `rust-toolchain.toml` files and `rust-version`. The container mounts all of `src/` (read-write, because the workspaces
  reach each other by path) at `/src`, works in `/src/<workspace>` and runs as the host user, so nothing root-owned
  appears. Named volumes keep the cargo registry (`workshop-rust-registry`, `workshop-rust-git`) and one build folder per
  workspace (`workshop-rust-target-<workspace>`, `CARGO_TARGET_DIR=/cargo-target`); no build output lands in the
  repository. The image sets `CARGO_PROFILE_DEV_DEBUG=0` (no debug info: the build output of `vaultspeed-framework`
  was over 20 GB with it and takes 2.5 times as long). Measured on a 20-core machine, all three workspaces cold
  (image built, volumes empty): `vdp-rust-utils` 33 s, `release-ui` 21 s, `vaultspeed-framework` 83 s, 2.5 minutes in
  all; warm, a few seconds each. The volumes hold about 7 GB, the image 3 GB. `CARGO_BUILD_JOBS=<n>` limits the compile parallelism like in the dev image build; `make rust-clean` removes
  the image and the volumes. The targets are not part of `make test` or `make bootstrap`. A host `cargo` with `rustup`
  still works (the pinned toolchain installs itself), at the price of large `target/` folders next to the sources.
- Workshop Studio: the `image.versions` action runs `vdp --version` (among other tools) in throwaway dev-image
  containers, and the scenario `same-toolbox` quotes the dev image's Dockerfile (from `RUN --mount=type=bind,source=src/sqlfmt`
  to `ln -s`). After editing that Dockerfile run `docker exec workshop-studio python -m studio.catalog --check`.
