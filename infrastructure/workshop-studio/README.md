# Workshop Studio

An interactive, zoomable map of the data value chain for driving the dbt workshop live:

```
① Source teams ──► ② Our data platform (4 engineering teams) ──► ③ Consumers
 webapp · Kafka · drive    Platform&Ingest · Core · Domain A · Domain B      Finance · Marketing · Management
 carrier · web analytics   (generated feeds, see below)
```

Start with the **happy sprint** (10 working days, everything green), **zoom into a layer**, **inject a scenario**,
**zoom out** to watch the impact ripple downstream, then **step through the resolution playbook**.

* **Sim** mode: pure simulation, no stack needed (laptop rehearsal).
* **Live** mode: every injection / playbook step with a real counterpart also runs against the stack
  (SingleStore fault injection, Airflow DAG triggers, GitLab branches / MRs / pipelines / tags). The map then shows the
  *observed* backend state (probes every 5 s) instead of the simulated one.

## Run it

```bash
infrastructure/workshop.sh up       # from the repo root: build + start the stack (also recreates Airflow)
infrastructure/workshop.sh reset    # once: schema, CI fixes, signing key, milestone (runs in the manager container)
xdg-open http://127.0.0.1:8085
```

### Bringing an existing stack up to date

Live mode depends on fixes outside the studio, and an already-running stack only picks them up like this:

| Fix | Lives in | How an existing stack gets it |
| :-- | :-- | :-- |
| CI `variables:` / `--vars` quoting / `after_script` cleanup, `tags: [critical]` + selectors, freshness, exposures | the dbt project, pushed to GitLab | full `workshop.sh reset` (re-pushes the project and the baseline tags) |
| `airflow-prod` could not run tasks (`AIRFLOW__CORE__EXECUTION_API_SERVER_URL`) | `compose/docker-compose.yml` | `workshop.sh up` (recreates the Airflow containers) |
| `dbt_source_freshness` task in `dbt_daily_load` | the dbt project's Airflow DAG | full `workshop.sh reset` (redeploys LDV/PRD DAGs) |
| GitLab CI job-token signing key (recent GitLab images: every runner job fails with `scheduler_failure`) | `manager/bootstrap_stack.py` | full `workshop.sh reset` (sets it if missing) |
| studio code, scenarios, patches | `workshop-studio/` | `docker compose up -d --build workshop-studio` |
| generated feeds (carrier, web), `late_pct` / `bot_events`, QA registry (LDV) | `singlestore/studio_schema*.sql` | full `workshop.sh reset` (`--reset-channels` applies the schema without reseeding) |
| carrier / web / DEV-only models, model swap + versions, severity switches, `tag:critical` in the daily-load selectors | the dbt project, pushed to GitLab | full `workshop.sh reset` |
| levers: `dbt_env_vars`, `dbt_daily_load_dbtignore`, JSON `disabled_dbt_models` | the dbt project's Airflow utils + DAG | full `workshop.sh reset` (redeploys LDV/PRD DAGs) |
| LDV/PRD workspaces in the volumes `workshop_dev` / `workshop_prod` (formerly the host folder `workshop_state/`); the warehouse's data volume mounted at `/data` | `compose/docker-compose.yml`, `utils/dbt_pod_utils.py` | `docker compose up -d --build`, then a full `bootstrap_stack.py` (the old stack's data was never in its volume) |
| milestone `baseline` (what Reset restores) | `manager/milestones.sh`, `manager/bootstrap_stack.py` | full `workshop.sh reset` (takes it), or `--milestone create baseline` |
| drift audit in `dbt_release` (gate) / `dbt_daily_load` (report only), E10 | the dbt project (`macros/ops/`, DAGs) | full `workshop.sh reset` (re-pushes the project, redeploys DAGs) |

Offline / development (no Docker stack needed for Sim mode):

```bash
# venv off the exFAT workshop drive (venv needs a symlink there)
python3 -m venv ~/.venvs/dbt-workshop && cd infrastructure/workshop-studio/backend && ~/.venvs/dbt-workshop/bin/pip install -r requirements-dev.txt
STATE_FILE=/tmp/studio.json ~/.venvs/dbt-workshop/bin/python -m app.main   # API on 127.0.0.1:8085
cd ../frontend && npm install && npm run dev                      # UI on http://localhost:5173 (proxies /api + /ws)
```

> **Security.** The studio needs the Docker socket, the GitLab root PAT and the SingleStore root password (all already in
> `bootstrap_stack.py`). It is published on `127.0.0.1:8085` only, and live actions run only after an explicit click:
> - **No keyboard live actions:** in Live mode (without dry-run) the `→` and `T` shortcuts never run anything against
>   the stack. A presenter clicker sends `→` too.
> - **Reset asks first:** a live Reset asks for confirmation. It restores the SingleStore milestone `baseline` (the
>   whole warehouse; SingleStore restarts) - or, without one, truncates the landing data -, force-pushes `dev`/`main`
>   to the baseline and deletes every other branch and tag, including participants' own branches. It also redeploys
>   the baseline code into the LDV and PRD workspaces (volumes `workshop_dev` / `workshop_prod`, mounted at
>   `/workshop_state/<env>`) and deletes the lever Variables.
> - **No requests from other pages:** other web pages open in the same browser cannot drive it. State-changing requests
>   need an `X-Studio: 1` header (with curl: `-H 'X-Studio: 1'`), which a cross-site page cannot add without a CORS
>   preflight the studio never grants. Unknown `Host` names and foreign `Origin`s are refused (DNS rebinding);
>   `STUDIO_ALLOWED_HOSTS` adds a host name.
>
> Use **dry-run** (header checkbox in Live mode) to render every command without executing anything. Never expose it.

## Presenter shortcuts

`→` run next playbook step · `T` next sprint day · `Esc` zoom out · `1` / `2` / `3` zoom to Sources / Platform / Consumers ·
**A+** = projector mode (large text). Clicking a layer's header zooms into it; clicking a node opens its knobs.
In Live mode `→` and `T` only show a reminder to click: running a step against the stack is always a deliberate click.
Slider knobs send their value when you let go (one SingleStore update, not one per drag step). In Live mode the
timeline marks the milestones of days 2-9 as `(sim)`: there a tick only trickles data, and the real merge / tag / release /
deploy is the matching H0 playbook step (the day's note says which).

## Scenarios

| ID | Layer | Scenario | Status |
| :-- | :-- | :-- | :-- |
| **H0** ★ | all | Happy sprint (feature → MR → dev → release → main → PRD) | live |
| **S1** ★ | sources | Kafka payments arrive late (`delay_batches`) | live |
| S2 | sources | Webapp contract break (`schema_variant=v2`, `fraud_hold`) | live (bonus) |
| **S3** ★ | sources | Reviews file delivered twice (`duplicate_pct`) | live |
| S4 | sources | Domain B not delivered in PRD | live (small) |
| **E1** ★ | engineering | Release collision: revert Domain B, conflict with Domain A | live |
| E2 · E3 · E4 | engineering | Column rename · full-refresh window · daily load during release | narrative + sim |
| **S5** ★ | sources | The carrier stops delivering: exclude list vs `.dbtignore` | live |
| **S6** ★ | sources | Carrier API v2 unannounced: switch the model version with an env var | live |
| **S7** ★ | sources | Two teams, two definitions of `customer_id` | live |
| **E5** ★ | engineering | Dirty data that only exists in DEV: dynamic severity + model swap | live |
| **E6** ★ | engineering | Late scans rewrite delivery history (arrival vs event order) | live |
| E7 | engineering | Source drift breaks a model nobody owns (`select *`, legacy node) | live |
| **E8** ★ | engineering | A crawler makes a naive model run out of memory | live |
| E9 | engineering | Transformation pitfalls that pass review | narrative + sim |
| **C1** ★ | consumers | Finance revenue double-counted (payments fan-out) | live |
| C2 · C3 | consumers | New metric mid-sprint · forecast skewed by late data | narrative + sim |

★ = fully implemented (phase 1: H0 S1 S3 E1 C1; phase 2: S5–S7, E5, E6, E8). Live scenarios need a particular starting state. Each declares it under `requires:`,
and in Live mode the studio checks it (read-only) **before** the injection runs anything. If a check fails, the injection
stops with HTTP 412 and a message that says what to do; nothing has run and the map is unchanged. The scenario cards
show the same requirements in Live mode.

| Scenario | Live starting state (checked) | How to get there |
| :-- | :-- | :-- |
| H0 | GitLab at the baseline: `dev` = `v1.0.0`, `main` = `2026_DROP1.0`, no other tags | **Reset** |
| E1 | GitLab at the baseline, and no reviews in PRD yet | **Reset** |
| C1 | Domain A in `dev` | H0 steps 1–2, or E1's injection |
| S3 | Domain B in `dev`, and the dedup fix not in `dev` yet | **Reset**, then E1's injection |
| S5 | no carrier API v2 payloads in the PRD landing table (S6 leaves them) | **Reset** after S6 |
| S7 · E7 | no crawler hits in the LDV landing table (E8 leaves them); S7: its fix not in `dev`; E7: `referrer` not renamed yet | **Reset** |
| E6 · E8 | their fix not in `dev` yet | **Reset** |

Run order without a Reset in between: S5 → S6 → E5 → E6 → S7 → E7 → E8 (S6 leaves v2 payloads in PRD: S5 first; E8
leaves crawler hits in LDV: last). After S6 a PRD daily load fails again once its version switch is cleared - Reset.

Long live steps (CI, tag and Airflow waits) can be stopped with **✕ Cancel** next to the busy indicator. The step stays
pending and the map unchanged, but live actions that already finished stay done, so running it again repeats the whole
step (branches are recreated, so that is safe). Tag waits also stop early with a "press Reset" message when CI tags the
branch head with another version than expected (for example `v1.2.0` instead of `v1.1.0`), instead of waiting 15 minutes.

## One YAML drives both worlds

`scenarios/world.yaml` (nodes, edges, 10-day sprint) and `scenarios/*.yaml` (scenarios) are the single source of truth.
The `sim:` blocks animate the map offline; the `live:` blocks run against the stack. The Code tab shows the exact
SQL / git / airflow command for every step. Edit the YAML and refresh the page: the folder is mounted into the
container and the studio re-reads it when the page loads (other open tabs follow). A broken edit is not loaded - the
page says why and the previous version stays active. (While an operation runs, the reload waits for the next refresh.)

```yaml
id: S1_kafka_late
layer: sources                    # sources | engineering | consumers | all
status: implemented               # or: narrative (simulation only)
focus: [channel.kafka, landing.raw_olist_order_payments]
requires:                         # optional, Live only: read-only check_* actions run before the injection
  - { action: check_gitlab_file, ref: dev, path: models/im_dimensional/dim_olist_payments.sql, hint: "run H0 first" }
inject:
  narrative: "..."
  sim:  { channel.kafka: { health: warn, knobs.delay_batches: 2 } }     # dotted keys: metrics.* / knobs.*
  live: [ { action: set_channel, env: prod, channel: kafka, delay_batches: 2 },
          { action: trickle, env: prod, days: 7 } ]
impact:                           # what you see when zooming back out (ripple = BFS depth from the injected node)
  - { node: exposure.finance_revenue_report, health: fail, note: "revenue -38% vs forecast" }
steps:
  - title: "Detect: source freshness"
    explain: "..."
    code: "dbt source freshness --select source:raw_olist.raw_olist_order_payments"
    zoom: platform                # layer or node to zoom to while presenting the step
    highlight: [landing.raw_olist_order_payments]
    sim:  { ... }                 # effects on nodes
    expect: { node: landing.raw_olist_order_payments, health: fail }   # Live: must be observed by a probe
```

Live actions run **before** the sim effects of a step, a knob change and a Reset: if the stack refuses (GitLab down,
pipeline red when green was expected), the step stays pending and the map is unchanged; the reason shows in the header
and the Log tab.

**Clearing or replacing a scenario** rolls back what it did: every node it touched returns to how it was right before
the injection (health, note, knobs, metrics), and in Live mode what it set in the stack is undone: channel faults
(`set_channel` anywhere in the scenario) back to healthy, lever Variables (`airflow_variable`) deleted, and `sql`
actions that declare an `undo:` statement reverted (E5's QA orders). Data that already landed, schema changes without an
`undo:` (E7's renamed column) and GitLab branches/tags stay - Reset for a clean stack. What is still set shows in the
header ("faults set: kafka@prod, disabled_dbt_models@prod").

**Step checks (`expect:`) in Live mode count only what a probe observed.** A step's own simulated effect does not count:
a check on a node no probe covers is reported as "not verifiable live" instead of passing. The probes ignore dbt test
results and Airflow runs from before the last live Reset, and they report a node only when one of its tests (or, for
nodes with `models:`, the model itself: error = fail, skipped = warn) actually has a result. In this stack LDV and PRD
build into the same model databases, so the latest result - whichever environment ran it - describes the table it checks;
per node only the newest run that touched it counts (after S6's version switch the old v1 failures no longer do).
`airflow.daily_load` shows the newest `dbt_daily_load` run of either environment ("LDV: ..." / "PRD: ..."), judged by
its `dbt_build` task; channels show the worse of their LDV and PRD fault settings; `airflow.levers` shows the lever
Variables set in each Airflow. Airflow runs and Variables are read from the Airflow metadata DB (one query), and the
`airflow` CLI is only a fallback.

| Action | Parameters | What it runs |
| :-- | :-- | :-- |
| `set_channel` | `env` dev\|prod\|both, `channel` webapp\|kafka\|drive\|carrier\|web, `enabled`/`delay_batches`/`duplicate_pct`/`partial_pct`/`null_key_pct`/`schema_variant`/`late_pct` (carrier)/`bot_events` (web) | `UPDATE _source_channel_config` |
| `trickle` | `env`, `days`, `domain_b` (default: dev yes; prod no until PRD has received reviews, then yes) | `CALL sp_trickle_olist_batch(days, domain_b)` |
| `reset_channels` / `reset_landing` / `sql` | `env`, `days` / `statement`, `undo`, `ignore_errors`, `undo_ignore_errors` | channel config reset · landing reset · raw SQL (`undo` runs on clear; listed MySQL error codes count as done, for GRANT/REVOKE GROUP and DROP ROLE that have no IF [NOT] EXISTS) |
| `milestone` | `op` create\|restore\|delete\|list, `name` | `manager/milestones.sh <op> <name>`: snapshot / restore of the stopped SingleStore data volume (Developer Edition: no `CREATE MILESTONE`, no `BACKUP`). API: `GET /api/milestones`, `POST /api/milestones/<name>/create\|restore\|delete` (Live mode) |
| `airflow_variable` | `env` dev\|prod\|both, `key`, `value` (lists/objects as JSON) or `delete: true` | `docker exec airflow-<env> airflow variables set\|delete` (deleted again on clear and Reset) |
| `airflow_trigger` | `env`, `dag`, `conf`, `wait`, `timeout`, `allow_failure` | `docker exec airflow-<env> airflow dags trigger … --run-id` + poll `list-runs` |
| `gitlab_branch` | `name`, `ref` | create branch (recreated if it exists) |
| `gitlab_commit_patch` | `patch`, `branch` | commits `patches/<patch>/` via the commits API |
| `gitlab_mr_merge` | `source`, `target`, `patch`, `timeout` | MR + squash-merge (retries until the MR pipeline allows it) |
| `gitlab_pipeline_wait` / `gitlab_tag_wait` / `gitlab_job_play` | `ref`+`expect` / `tag` / `ref`+`job` | wait for pipeline (failed job log tail shown) / tag / play a manual job |
| `git_script` | `revert_on_release` · `promote_release` · `reset_branches` | the exact git commands of the release-flow skill in a temp clone |
| `reset` | `milestone` (default `baseline`) | warehouse back to the milestone (fallback without one: channels + landing re-seeded) + GitLab back to baseline; baseline code (`v1.0.0` / `2026_DROP1.0`, GitLab archive) redeployed to the LDV / PRD workspaces (`dbt_packages` kept); lever Variables deleted. Rebuilding every model from scratch stays `reset_workshop.py` |
| `check_gitlab_baseline` / `check_gitlab_file` / `check_sql` | `hint` / `ref`, `path`, `exists`, `not_contains` / `env`, `statement`, `expect` | `requires:` only: GitLab at the baseline · a file on a branch · a scalar query result |

## Per-channel fault injection (SingleStore)

`singlestore/studio_schema.sql` adds `_source_channel_config`, `_delivery_log`, `_held_batches` and rewrites
`sp_trickle_olist_batch(days_step, load_domain_b_reviews)` (same signature; with the default config the landed rows are
identical to the original procedure - covered by a regression test). Faults are deterministic (CRC32 of the business key).
CLI equivalents of every knob:

```bash
infrastructure/workshop.sh reset --channel kafka --delay-batches 2 --env prod   # late payments
infrastructure/workshop.sh reset --channel drive --duplicate-pct 100            # same file twice
infrastructure/workshop.sh reset --channel webapp --schema-variant v2           # new order_status 'fraud_hold'
infrastructure/workshop.sh reset --show-channels                                # config + delivery log
infrastructure/workshop.sh reset --reset-channels                               # healthy again
```

dbt makes the struggles detectable: source freshness (relative to the simulation watermark, see
`macros/utils/olist_freshness.sql`; the `dbt_source_freshness` task of `dbt_daily_load` reports it after every build
and never fails the load), source tests, exposures for the three consumers, and the patch sets in
`patches/` (Domain A/B features, the finance fan-out bug + fix, reviews dedup, webapp contract v2). Patch sets are
committed to *feature branches* through the GitLab API - `main` is never touched directly.

## Generated feeds (beyond Olist)

Olist has no parcel scans, no clickstream and no second team that defines "customer" differently. Two generated feeds
fill that gap; they are derived deterministically from `_olist_master` (built once by `sp_build_generated_archives`
on the first landing reset), so they join with the Olist feeds the way real sources would:

| Channel | Team | Landing table | Generated from | Faults that matter |
| :-- | :-- | :-- | :-- | :-- |
| `carrier` | Carrier (logistics partner) | `raw_carrier_tracking_events` (`payload` JSON) | pick-up / in-transit / delivery scans from the Olist carrier + customer dates | `schema_variant` v2 = API v2 (camelCase, nested status code, UTC); `late_pct` = non-final scans uploaded 2 batches late; `delay_batches` = outage with backlog |
| `web` | Web-analytics team | `raw_web_events` | 2-5 page views + cart + checkout before every purchase, anonymous browsing, one cookie per person | `schema_variant` v2 = `customer_ref` is the web account (the person) instead of the per-order webshop id; `bot_events` = a crawler on one cookie |

DEV-only: `analytics_dev._qa_test_accounts` and `CALL sp_qa_smoke_test(n)` (QA test orders on the wall clock, on accounts
outside the customer feed), from `singlestore/studio_schema_dev.sql`. Every trickle batch now gets its own, later
`_ingested_at` (also when batches run within one second), so arrival order stays visible to the models.

## dbt patterns and Airflow levers (S5–S7, E5–E8)

| Pattern | Where | Scenario |
| :-- | :-- | :-- |
| Global test severity switch | `dbt_project.yml`: `+severity: "{{ env_var('DBT_TEST_SEVERITY', 'warn') }}"` | E5 |
| Local severity switch | `sources_olist.yml`, `tests/olist/assert_orders_not_after_simulation_clock.sql`: warn in LDV/ABT, error in PRD/CBT, `DBT_SEVERITY_DEV_DATA` overrides | E5 |
| Model swap (same name, dynamic `enabled`) | `models/edw_dv_bdv/stg_olist_orders.sql` (PRD/CBT) and `models/edw_dv_bdv/dev_only/stg_olist_orders.sql` (LDV/ABT, filters QA orders via a DEV-only source) | E5 |
| Dynamic model version resolution | `models/edw_dv_bdv/carrier/carrier.yml` (v1/v2, `enabled` per version) + `ref('stg_carrier_tracking_events', v=env_var('DBT_VERSION_STG_CARRIER_TRACKING_EVENTS', '1'))` | S6 |
| `.dbtignore` from an Airflow Variable | `dbt_daily_load_dbtignore` (JSON list): appended to `.dbtignore` before `dbt_build` and `dbt_source_freshness` start, restored afterwards | S5 |
| Exclude list from an Airflow Variable | `disabled_dbt_models` (JSON list or comma/space separated): appended to `--exclude`; nodes stay in the lineage | S5, E7, E8 |
| Env vars from an Airflow Variable | `dbt_env_vars` (JSON object): exported before every dbt task of every DAG; secrets and `DBT_DEPLOY_TARGET` refused | S6, E5 |

The Airflow side lives in `airflow/utils/dbt_runtime_overrides.py` of the dbt project (tests: `airflow/tests/`, no Airflow
installation needed: `python -m pytest tests` from `airflow/`). The `daily_load` selectors now also run every test tagged
`critical` - before, tests on sources (source contracts, delivery guards) were never selected, because indirect selection
only reaches tests whose parents are all selected models.

Measured on the test SingleStore (6 GB, like the workshop node): the naive `mart_web_journeys` fails with error 1712
(out of memory) after ~27 s with 30,000 crawler hits and takes ~1 s after the E8 fix; dbt Fusion does not turn a
warn-severity test into a failure under `--warn-error` (dbt Core does).

## Tests

```bash
cd workshop-studio/backend && pytest                        # content validation, sim playbooks end green, dry-run rendering,
                                                            # git revert/promote on a local repo, REST + websocket
STUDIO_TEST_S2_PORT=13306 pytest tests/test_sql_trickle.py  # SQL regression + fault injection against a throwaway SingleStore
cd workshop-studio/frontend && npm run build && npm test    # (on exFAT/NTFS drives without symlinks: ./build-check.sh)
```

### What was verified live

**Self-contained stack, run through the manager container (2026-11-07):** `workshop.sh down -v` + `workshop.sh bootstrap`
on a fresh machine state (no host Python) completed: seed (514,632 Olist rows via `LOAD DATA LOCAL INFILE`), GitLab,
runner, both deployments into the `workshop_dev` / `workshop_prod` volumes, docs on 8081/8082, milestone `baseline`
(2.4 GB). **E10** ran live, 5/5 steps: the PRD daily load's report-only audit listed `[changed outside dbt]
fct_olist_orders` and `[created outside dbt] v_delivery_sla_hotfix` and the build erased the hotfix; the `dbt_release`
gate failed on `[missing grant] svc_prd_dbt lacks DROP, ALTER on im_dimensional` before pausing or building anything;
after the fix the audit reported 0 findings. `workshop.sh reset` passed, and the studio Reset restored the milestone
(28 s in total, a table created after the milestone was gone).

All five phase-1 scenarios (H0, S1, S3, E1, C1) were run end to end through the studio in **Live mode against a complete, freshly bootstrapped stack**
(`docker compose up` + `bootstrap_stack.py` in an isolated copy: SingleStore, GitLab CE + runner, MySQL, airflow-dev/prod, dbt images):
real MRs with `verify_feature_to_dev`, `tag_dev`/`tag_main` tags, `publish_dev_artifacts`, `verify_release_branch` going red on the Domain B guard and
green after the revert (real conflict, resolved), promote, `deploy_prd_manual` and `dbt_release` (incl. the deferred `dbt_reload`).
The probes observed the Kafka lag (S1) and the finance reconciliation failing, then passing (C1). For S3 that run only showed that every
action succeeded and the playbook reached "resolved": the duplicate reviews were **not** observed, because nothing ran dbt between the
injection and the detect step. The SQL suite also ran against that stack's SingleStore.
Not run live: the phase-2 narrative scenarios (E2, E3, E4, C2, C3) - simulation only by design.

**Second live run (fresh isolated stack, after the starting-state / Cancel / S3 / Domain A fixes):** H0 7/7, E1 6/6, S3 4/4 and
C1 4/4 resolved, and all 9 step expectations were met by *observed* state:
- **S3:** `dbt.stg_olist_reviews` fail, then ok. `dbt_run_results` shows the unique test passing before the duplicates and failing
  after them with 861 duplicate keys.
- **C1:** the finance report fail, then ok.
- **E1:** `ci.verify` fail, then ok. The revert of Domain B still conflicts in `fct_olist_orders.sql`; `release/2026-Q1` keeps only
  `domain_a_hkey`, and `main` after Q2 has both columns.
- **H0:** promotes a `main` whose `fct_olist_orders.sql` has `domain_a_hkey` only (the old full-file Domain A patch also leaked
  Domain B's column).
- **Waits for `publish_dev_artifacts`:** the S3 and C1 rebuild steps both found the fix's tag pipeline still running, so the wait
  is needed.
- **Refusals:** without a Reset, E1 and S3 were refused with HTTP 412 before anything ran, and the map was unchanged.
- **Cancel:** stopped a 10-minute tag wait at once (409, step left pending), and the next action ran normally.

That run also found one bug, fixed and tested: S3's "dedup fix not in dev" check failed when the file did not exist at all.

**Third live run (smoke test of the safety and probe fixes, fresh isolated stack):**
- **Image:** the studio image builds with `npm ci` and the pinned Python versions.
- **Request guard:** the real server refuses a POST without `X-Studio`, a foreign `Origin` and a foreign `Host` (403).
- **S1:** resolved with all 3 checks observed (landing fail, landing ok, prod `dbt_daily_load` ok).
- **Airflow runs:** read from the metadata DB inside the studio container.
- **Freshness task:** `dbt_daily_load` lists and runs `dbt_source_freshness`. It flagged the withheld PRD reviews as stale,
  and the load still finished green.
- **Clear:** clearing S1 in Live mode put Kafka's `delay_batches` back from 2 to 0 in SingleStore.
- **Reset:** a live Reset hid the pre-reset `dbt_daily_load` run from the map.
- **Reviews:** after S4 delivered reviews to PRD, the next sprint tick kept delivering them (1,591 → 1,750 rows).
- **YAML reload:** an edit to a scenario file was picked up on the next page load.

**Fourth live run (after switching the dbt image, see `dbt-singlestore-base-image/BUILD-INFO.txt`):** the image now runs
dbt-oss 2.0.8 built from `/opt/dbt` `feat/backfill-hardening-pr`. That build connects through the SingleStore ADBC driver,
which ships in the image at `/usr/local/lib/dbt/`. On a fresh isolated stack:
- **Bootstrap:** the initial PRD and LDV full builds ran 27/27 nodes.
- **Scenarios:** H0 (incl. `dbt_release` and its `dbt_reload`), E1, S3, C1 and S1 all resolved, with all 12 step checks
  observed. Every CI job ran in the new image.
- **Behavioural difference:** in that run S1's prod `dbt_build` failed on 682 duplicated reviews that S3 had put in PRD.
  The new binary is right: the dedup fix had only reached `dev`. The `dbt_daily_load` run still ended "success",
  because `persist_artifacts_to_state` runs on `all_done`.
- **Follow-up fix:** the studio now judges `dbt_daily_load` by its `dbt_build` task (`KEY_TASKS` in
  `actions/airflow.py`), both in the Airflow probe and in `airflow_trigger`. Re-run live (E1 → S3 → S1): the map shows
  the daily load as `fail` ("dbt_build failed (the run itself ended success)"), the trigger logs ⚠ instead of ✓, and
  S1's check on `airflow.daily_load` reports the failure instead of passing.

**Phase-2 run (generated feeds, levers and patterns; fresh isolated stack, then two re-runs after a Reset):**
- **Bootstrap:** the initial PRD and LDV full builds ran 51/51 and 55/55 nodes (carrier, web and DEV-only models included).
- **Regression:** H0 7/7 (incl. `verify_release_branch`, `dbt_release`) before the new scenarios, and H0 7/7 + C1 4/4 after the new
  Reset.
- **S5 · S6 · E5 · E6 · S7 · E7 · E8:** all resolved; on the final run all 25 step checks were met by observed state.
  - S5: the guard failed the PRD load; `disabled_dbt_models` made it green; the partial `.dbtignore` broke the parse
    (`dbt1048 Ref 'fct_olist_deliveries' not found` in the `dbt_build` log); the full list made it green; the carrier backlog
    landed and the freshness probe went back to ok. No `.dbtignore` was left in the workspace afterwards.
  - S6: v1 `not_null` failed on API v2 payloads; with `dbt_env_vars` the Airflow log shows `export
    DBT_VERSION_STG_CARRIER_TRACKING_EVENTS=2` in front of `dbt_build`, `dbt_source_freshness` and `persist_artifacts_to_state`,
    and the PRD build ran 51/51 on v2.
  - E5: the QA orders warned in LDV, failed with `DBT_SEVERITY_DEV_DATA=error`, and the strict PRD load stayed green.
  - E6: delivered parcels flipped back to in-transit after late scans (`assert_delivery_status_consistent` failed); green after
    the event-time fix.
  - S7: attribution fell below the 90% threshold after the web team's v2 `customer_ref` (34% on the test database); green
    after the fix.
  - E7: the renamed `referrer` broke only the legacy `int_web_top_pages`; green with the exclude, then with the staging contract.
  - E8: `mart_web_journeys` failed with **error 1712, memory reached `maximum_memory`** on the 6 GB workshop node; green with the
    exclude, then in ~1 s with the fix.
  - Clear: every lever Variable deleted in both Airflows (`airflow variables list`: none), nothing left pending.
- **Map semantics:** a channel that keeps a harmless fault (S6's v2 contract, E6's late scans, S7's v2 `customer_ref`) ripples `warn`
  downstream, even when the nodes' own tests pass. Those scenarios' last checks therefore look at `airflow.daily_load`, which is
  judged by `dbt_build` and so includes the critical tests.

Running it for real surfaced problems that no mock would have, all fixed in this change:

* **Studio:** `list-runs` JSON hidden behind Airflow log lines; `reset_channels` missing `USE`; Reset's force-push of `dev` triggered `tag_dev` and tagged the
  baseline `v1.1.0` (now `-o ci.skip`; tag waits can require the tag to sit on a branch head); `manual`-only pipelines count as finished; pipeline lookups
  filter by ref (a tag pipeline shares the SHA); `gitlab_job_play` is idempotent and waits; `gitlab_mr_merge` waits for a green MR pipeline;
  GitLab's squash-merge leaves a merge commit (revert needs `-m 1`); baseline tags are annotated.
* **Existing stack/CI config (not studio code):** `.gitlab-ci.yml` used `${VAR:-default}` in `variables:` (GitLab leaves them empty -> every dbt job failed) and
  `--vars '"SCHEMA: x"'` (rejected by the dbt binary); failed verify jobs left half-built `vdp_ci` tables that broke the next run (`after_script` cleanup +
  `drop_nodes` also drops `__dbt_tmp`); `airflow-prod` could not run any task (`AIRFLOW__API__BASE_URL` pointed at the published port; now
  `AIRFLOW__CORE__EXECUTION_API_SERVER_URL`); DAGs start paused (the studio unpauses what it triggers, incl. `dbt_reload` for `dbt_release`); recent GitLab images need
  the CI job-token signing key (set in `bootstrap_gitlab_rails`); the project's default selector only runs `unique_combination_of_columns` tests, so guard /
  reconciliation / source tests opt in with `tags: [critical]` (added to `selectors.yml`).
* **Reset and deployments (fixed in the phase-2 run):** the studio's Reset rolled GitLab back but left LDV running whatever the last
  `tag_dev` had published - a scenario's merged fix (S3, C1, E6, S7, E7, E8) stayed live, so re-running the scenario had nothing left to
  detect (E6's injection passed its own check) or broke on restored data (E7's staging fix vs the restored `referrer` column). Reset now
  redeploys the baseline tags into the workspaces; verified live by re-running all seven phase-2 scenarios and H0 + C1 after it.

C1's injection merges an MR with a red pipeline on purpose (`wait_pipeline: false`): this project does not require a green pipeline to merge, which is part of the story.
