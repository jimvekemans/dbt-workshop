# dbt × SingleStore workshop

A hands-on workshop on running a dbt data platform **as a team of teams**: what breaks between the source teams that
deliver data, the engineering teams that transform it, and the business that reads it, and how dbt, CI and Airflow
let you detect, contain and fix it without stopping the platform.

Everything runs locally in Docker: a SingleStore warehouse with two environments, GitLab with CI, two Airflow
instances (LDV and PRD), dbt docs and the **Workshop Studio**, an interactive map used to drive the scenarios live.

## Goal

By the end of the workshop participants can:

1. **Follow a change end to end:** feature branch → MR → `dev` → `release/YYYY-Qx` → `main` → PRD, and know which
   CI job and which Airflow DAG guards each step.
2. **Recognise the three layers of failure:** a source team delivers late, wrong or nothing; a transformation is
   subtly wrong; a consumer reads a number that is wrong without anything failing.
3. **Detect before the business does:** source contracts, freshness, critical tests and reconciliation tests.
4. **Contain without a deploy:** switch versions, test severity and model selection at runtime with environment
   variables and Airflow Variables.
5. **Fix through the release flow:** bugfix branch, MR, rebuild, and see the same fix reach PRD.

## Stakeholders

| Layer | Who | What they own |
| :-- | :-- | :-- |
| ① Sources | **Webapp team** | the webshop: orders + customers (CDC feed) |
| | **Kafka team** | the payments topic |
| | **Mounted-drive team** | review files on a mounted drive |
| | **Carrier** (external) | parcel tracking scans via an API (generated feed) |
| | **Web-analytics team** | the webshop clickstream (generated feed) |
| | **QA team** | smoke-test orders, in DEV only |
| ② Platform | **Platform & Ingest** | sources, `stg_*`, Airflow DAGs, CI, release / reload |
| | **Core model** | `dim_olist_customers`, `fct_olist_orders`, `datum` |
| | **Domain A squad** | payments → Finance |
| | **Domain B squad** | reviews → Marketing |
| ③ Consumers | **Finance** | revenue report |
| | **Marketing** | NPS dashboard, web conversion |
| | **Management** | KPIs, forecast, delivery KPI |

## Infrastructure

```
              ┌──────────────── workshop_net (Docker) ────────────────┐
 Source feeds │  SingleStore  analytics_dev ─┐        ┌─ analytics_prd │
 (trickle)  ──┼─►  landing      (LDV)        │        │   (PRD)        │
              │                 ▼            ▼        ▼                │
              │   dbt Fusion  edw_dv_rdv → edw_dv_bdv → im_dimensional │
              │                 ▲                        → pres_*      │
              │   airflow-dev (LDV)            airflow-prod (PRD)      │
              │   dbt_daily_load · dbt_release · dbt_reload · trickle  │
              │                 ▲                        ▲             │
              │   GitLab + runner: verify → tag → publish → deploy     │
              │   dbt-docs (LDV / PRD)     Workshop Studio (map)       │
              └────────────────────────────────────────────────────────┘
```

| Service | URL | Purpose |
| :-- | :-- | :-- |
| Workshop Studio | http://127.0.0.1:8085 | the live map: inject scenarios, step through the playbook |
| GitLab | http://localhost | repo `workshop/dbt-vdp`, CI pipelines, MRs |
| Airflow LDV | http://localhost:8088 | `workshop-airflow-dev`, service user `svc_ldv_dbt` |
| Airflow PRD | http://localhost:8090 | `workshop-airflow-prod`, service user `svc_prd_dbt` |
| dbt docs | http://localhost:8081 (LDV) · :8082 (PRD) | lineage, exposures |
| SingleStore | `localhost:23306` · Studio UI :8083 | the warehouse (inside the stack: `singlestore:3306`) |

**The stack is self-contained.** Every container is named `workshop-<service>`, and the volumes and network carry a
workshop prefix too. Host ports stay clear of a personal SingleStore (3306, UI 8080). Change them with
`WORKSHOP_S2_PORT`, `WORKSHOP_S2_UI_PORT`, `WORKSHOP_AIRFLOW_LDV_PORT` or `WORKSHOP_AIRFLOW_PRD_PORT`.

Credentials are in [bootstrap_stack.py](infrastructure/manager/bootstrap_stack.py) and the
[architecture skill](.agents/skills/vdab-dbt-workshop-architecture/SKILL.md).

**State lives in Docker volumes, not in the repo.** The deployed LDV and PRD workspaces (DAGs, dbt project, docs,
`/mnt/dbt` artifacts) are the volumes `workshop_dev` and `workshop_prod`. Bootstrap, the CI deploy job and the
studio Reset fill them. The warehouse is `workshop_s2_data`. Its **milestones** (named restore points of the whole
warehouse) are in `workshop_s2_milestones`. Bootstrap takes `baseline`, and the studio Reset restores it.

### Release flow (Modified GitFlow)

| Branch / tag | Built by | Runs in |
| :-- | :-- | :-- |
| `feature/*` → MR to `dev` | `verify_feature_to_dev` (`state:modified+`) | — |
| `dev` | `tag_dev`, `publish_dev_artifacts` | LDV (`dbt_daily_load`) |
| `release/YYYY-Qx` | `verify_release_branch`, ABT / CBT deploys | acceptance |
| `main` | `promote_release_to_main`, `tag_main`, `deploy_prd_manual` | PRD (`dbt_release` → `dbt_daily_load`) |

Details: [dbt-release-flow skill](.agents/skills/dbt-release-flow/SKILL.md) and
[dbt-reload-pipeline skill](.agents/skills/dbt-reload-pipeline/SKILL.md).

### Data

The business is the public **Olist** e-commerce dataset (orders, customers, payments, reviews), replayed day by day
(`ifw_olist_trickle`) on a 2017 business clock. Olist has no parcel scans, no clickstream and no second team with its own
definition of "customer", so two feeds are **generated from it** and join with it like real sources:

- **Carrier tracking API:** a JSON payload per pick-up / in-transit / delivery scan. Can switch to API v2, upload scans
  late, or stop delivering.
- **Web clickstream:** page views, cart and checkout before every purchase. Its customer reference can switch from the
  per-order id to the person; a crawler can be switched on.
- **DEV only:** QA smoke-test orders on test accounts (`analytics_dev` only).

Every feed has fault knobs (delay, duplicates, null keys, schema variant, outage, late %, bots), set per environment from
the studio or `infrastructure/workshop.sh reset --channel`.

## Scenarios

★ = runs live against the stack (the others are simulation / discussion).

| ID | Layer | Scenario | Teaches |
| :-- | :-- | :-- | :-- |
| **H0** ★ | all | Happy sprint: one feature from MR to the CFO's report | the release flow, end to end |
| **S1** ★ | sources | Kafka payments arrive late | source freshness, silently incomplete revenue |
| S2 | sources | Webapp contract break: new `order_status` | accepted_values as a source contract |
| **S3** ★ | sources | Reviews file delivered twice | uniqueness at staging, NPS spike |
| S4 | sources | Domain B not delivered in PRD | a source that exists in LDV only |
| **S5** ★ | sources | The carrier stops delivering | critical guard; `disabled_dbt_models` and `.dbtignore` levers |
| **S6** ★ | sources | Carrier switches to API v2, unannounced | model versions, version picked by env var |
| **S7** ★ | sources | Two teams, two definitions of `customer_id` | join-key contracts, attribution collapse |
| **E1** ★ | engineering | Release collision: revert Domain B, keep Domain A | `git revert` conflicts on a release branch |
| E2 | engineering | Core renames a column Domain A uses | `state:modified+` in CI |
| E3 | engineering | Full refresh blows the nightly window | blue-green reload, batching |
| E4 | engineering | Daily load runs during a release | DAG coordination |
| **E5** ★ | engineering | Dirty data that only exists in DEV | dynamic test severity, model swapping |
| **E6** ★ | engineering | Late scans rewrite delivery history | arrival order ≠ event order |
| E7 | engineering | Source drift breaks a model nobody owns | lazy `select *`, unowned nodes |
| **E8** ★ | engineering | A crawler makes a naive model run out of memory | quadratic joins, real OOM (error 1712) |
| E9 | engineering | Transformation pitfalls that pass review | dedup, watermarks, `NOT IN`, casts, time zones |
| **E10** ★ | engineering | A hotfix nobody committed, a grant nobody copied | drift audit: live PRD vs Git, release gate |
| **C1** ★ | consumers | Finance revenue double-counted | fan-out, reconciliation tests |
| C2 | consumers | Marketing wants a new metric mid-sprint | the cost of a quarterly release |
| C3 | consumers | Forecast skewed by late data | consequences of S1 downstream |

Each scenario follows the same rhythm: **inject → zoom out to see the impact → detect → analyse → fix → rebuild**.

### Suggested agenda (full day)

| Block | Scenarios |
| :-- | :-- |
| Intro + the happy path | H0 |
| Sources deliver late, double, wrong | S1, S3, S2 |
| Sources go silent or change shape | S5, S6, S7 |
| Release flow under pressure | E1, E2 |
| Environments and transformation traps | E5, E6, E7, E8, E9, E10 |
| What the business sees | C1, C3, C2 |

Without a Reset in between, run the phase-2 scenarios in this order: S5 → S6 → E5 → E6 → S7 → E7 → E8. E10 only
touches PRD's service account and one model table, so it fits anywhere; its fix step restores the account. Every live
scenario checks its starting state and refuses with a hint if it is not met.

## dbt patterns and Airflow levers

| Pattern | Where |
| :-- | :-- |
| Global test severity switch (`DBT_TEST_SEVERITY`) | `dbt_project.yml` |
| Local severity switch per environment (`DBT_SEVERITY_DEV_DATA`) | `models/edw_dv_bdv/sources_olist.yml` |
| Model swap: two `stg_olist_orders`, one enabled per environment | `models/edw_dv_bdv/stg_olist_orders.sql`, `models/edw_dv_bdv/dev_only/` |
| Model version picked by env var (`DBT_VERSION_STG_CARRIER_TRACKING_EVENTS`) | `models/edw_dv_bdv/carrier/carrier.yml`, `fct_olist_deliveries.sql` |
| Airflow Variable `dbt_env_vars`: env vars exported before every dbt task | `airflow/utils/dbt_runtime_overrides.py` |
| Airflow Variable `dbt_daily_load_dbtignore`: written to `.dbtignore` for the daily load | `airflow/dags/dbt_daily_load.py` |
| Airflow Variable `disabled_dbt_models`: appended to `--exclude` | `airflow/utils/dbt_command_utils.py` |
| Drift audit: live warehouse vs Git (changed or created outside dbt, missing grants). It is the release gate in `dbt_release` and runs report-only before `dbt_build` | `macros/ops/audit_environment_drift.sql` |

Two things to know on this dbt Fusion build: `--warn-error` does **not** promote a warn-severity test (the severity
decides), and a `.dbtignore` entry that leaves a dangling `ref` fails the whole parse.

## Run it

The host needs only Docker with the compose plugin. All the Python (bootstrap, reset, trickle, channel faults,
milestones) runs in the **manager container** (`infrastructure/manager/`), which drives the other containers through
the Docker socket. [workshop.sh](infrastructure/workshop.sh) is the one entry point:

```bash
infrastructure/workshop.sh bootstrap      # first time: build + start the stack, then provision everything (~15 min)
```
```bash
infrastructure/workshop.sh reset          # full rebuild back to the baseline (+ fresh milestone)
```
```bash
infrastructure/workshop.sh up             # start / update the stack (docker compose up -d --build)
```
```bash
xdg-open http://127.0.0.1:8085
```

- `workshop.sh reset --show-channels` prints the feed fault settings, and `workshop.sh reset --channel <name> --env dev|prod ...`
  sets them. `workshop.sh shell` opens bash in the manager. `workshop.sh stop`, `ps` and `logs` pass through to compose.
- The studio's **Reset** button restores the SingleStore milestone `baseline`: every landing and model table, the run
  log, users and grants. It also resets GitLab, redeploys the baseline code into LDV and PRD and deletes the levers.
  SingleStore restarts during the restore (about 15 s).
- Your own restore points: `workshop.sh milestone create|restore|delete <name>` or `workshop.sh milestone list`, or
  `POST /api/milestones/<name>/create|restore` on the studio. The SingleStore Developer Edition has no native
  `CREATE MILESTONE` or `BACKUP`, so a milestone is a snapshot of the stopped data volume
  ([milestones.sh](infrastructure/manager/milestones.sh)).
- Opening `dbt-singlestore-workshop/` in VS Code (or its devcontainer) starts the cluster through
  `workshop.sh session` and stops it when the last window closes.
- Sim mode needs no stack at all, which makes it good for rehearsing on a laptop.

More: [workshop-studio/README.md](infrastructure/workshop-studio/README.md) (studio, scenario requirements, live
verification), [TODO.md](TODO.md) (open items).

## Repository layout

| Path | Contents |
| :-- | :-- |
| `dbt-singlestore-workshop/` | the dbt project, its Airflow DAGs/utils and `.gitlab-ci.yml` (pushed to GitLab) |
| `infrastructure/workshop.sh` | the one entry point: `up`, `bootstrap`, `reset`, `milestone`, `shell`, `session` (VS Code), `stop`/`ps`/`logs` |
| `infrastructure/compose/` | `docker-compose.yml` (every service, pinned upstream images) and `.env` (GitLab OAuth credentials, written by bootstrap) |
| `infrastructure/manager/` | the manager image and everything it runs: `bootstrap_stack.py`, `reset_workshop.py`, `milestones.sh` |
| `infrastructure/gitlab/` | `gitlab.rb`: the GitLab Omnibus config (the image itself is upstream `gitlab/gitlab-ce`) |
| `infrastructure/airflow/` | the Airflow image, plus `mysql/` (its metadata DB init) |
| `infrastructure/singlestore/` | landing schema, generated feeds, fault-injection procedures, `olist_archive/` (downloaded) |
| `infrastructure/workshop-studio/` | the studio: backend, frontend, scenarios, patches |
| `infrastructure/dbt-singlestore-base-image/` | the dbt Fusion + SingleStore ADBC driver image |
| `infrastructure/nginx/` | the dbt-docs server config |
| `.agents/skills/` | workshop skills: architecture, release flow, reload pipeline |

The workshop presentation (goal, infrastructure, stakeholders, scenarios) is published as a slide deck:
https://agents.ai/artifact/LUuYnU8oGvZSCkKFWnwVNj (private until shared from its Share menu; exports to PPTX / PDF).
