# dbt Cloud features the workshop lacks: plan of approach

Written 2026-11-22. It covers the eleven dbt Cloud features the stack does not have, and says for each whether it
can be built here, how, and in which order. This is a plan only: nothing in the stack was changed for it.

**Result:** six features can be built with modest work, mostly on things the stack already produces. Three need a
trial first, because they depend on engine behaviour nobody has run on SingleStore. Two are not worth building.

**How it was made:** from the skills, the CI file, the DAGs and the deployed dev docs site, plus a look at the flags
and crates of the dbt engine in the base image. The dbt Cloud side is from memory, not from their current
documentation. Effort is a rough guess: S is a day or two, M about a week, L several weeks.

## What the research found

These five facts shape the plan. Each was read from the repository or the running stack; none was tried out.

| Finding | Where | Consequence |
|---|---|---|
| The docs site is already the new generation: a static page over parquet files, the engine's "information schema" (models, columns, edges, exposures, run results, freshness, column lineage) with DuckDB views in `views.sql` | `workshop_dev:/docs/info_schema/v1/`, served at `http://localhost:8081/info_schema/v1/` (HTTP 200) | A metadata API and a catalogue need no new data source |
| `dbt.column_lineage.parquet` is empty (885 bytes) and `dbt_rt.run_results.parquet` holds only the docs run | same folder | Column lineage and run health are one deploy change away, if strict analysis works |
| Column lineage needs `--static-analysis strict`; the engine knows a SingleStore dialect, but strict mode on this project is untested | `crates/dbt-docs-server/README.md`, `crates/dbt-common/src/adapter.rs` | Trial 1 below |
| The engine has a MetricFlow compiler, but its dialects are DuckDB, Snowflake, Redshift, BigQuery and Databricks | `crates/dbt-metricflow/src/lib.rs` | No semantic layer without writing a SingleStore dialect |
| The engine has `--state-mode`, a hidden `--manage-state` ("service-backed dbt State") and the selector `source_status:` | `crates/dbt-clap-core/src/lib.rs`, `crates/dbt-common/src/node_selector.rs` | State-aware runs may work in part without the dbt platform: trial 2 |

Crate paths are relative to `infrastructure/dbt-singlestore-base-image/src/dbt/`.

## Overview

| # | Feature | Verdict | Effort | Phase |
|---|---|---|---|---|
| 1 | Catalog: column lineage, run health in the docs | Feasible after trial 1 | S | 1 |
| 2 | Discovery API (metadata) | Feasible, the data is already served | S | 1 |
| 3 | Admin API (start and inspect jobs) | Exists in Airflow and GitLab, needs an account and a page of documentation | S | 1 |
| 4 | Webhooks | Feasible | S to M | 1 |
| 5 | CI "compare changes" | Feasible | M | 2 |
| 6 | Audit log | Feasible as a collected view, not tamper-proof | M | 2 |
| 7 | Copilot | Feasible, but a policy decision first | S to M | 2 |
| 8 | State-aware orchestration | Partly, after trial 2 | M | 3 |
| 9 | Cross-project references (dbt Mesh) | Feasible in a reduced form | L | 3 |
| 10 | Semantic Layer | Only by extending the dbt fork | L | 3, or skip |
| 11 | Insights (query screen) | Covered well enough today | none | skip |
| 12 | Canvas (visual editor) | Not feasible at a sensible cost | none | skip |

## Phase 1: built on what the stack already produces

### 1. Catalog: column lineage and run health

1. **Trial 1.** On a throwaway SingleStore (architecture skill §11), run `dbt compile --static-analysis strict` with
   the index on, then `dbt docs generate --no-compile`, as the docs-server README describes. Note which models the
   strict analysis rejects (SingleStore syntax such as `:>` casts is the likely cause).
2. If most models pass: change step 2 of `.deploy` in `.gitlab-ci.yml` to the two-step form. Models that fail get
   the analysis switched off one by one, so the rest keep their lineage.
3. Run health: `dbt_daily_load` already builds with the index. Add a task after `persist_artifacts_to_state` that
   runs `dbt docs generate --no-compile --output-dir` into the `docs` folder, so the site shows last night's status
   and freshness. It needs the `docs` subpath mounted for that one task.

Risk: if strict analysis fails on most models, only step 3 remains.

### 2. Discovery API

The parquet files are the API's content already. Two levels:

- **Document what is there** (S): the folder URL per environment, `views.sql`, and three example queries with
  DuckDB (`read_parquet('http://localhost:8081/info_schema/v1/dbt.models.parquet')`). Add it to the docs front page
  and the overview page. Run history is in `<env>_vdp_mgmt.dbt_run_results` and the models built on it
  (`dbt_table_performance`, `dbt_active_test_failures`), readable with the `E_reader` roles.
- **A small read-only service** (M), only when a consumer cannot read parquet: a container shaped like
  `infrastructure/overview/` that answers JSON for models, lineage, last run, freshness and deployed version.

### 3. Admin API

Airflow 3 has a REST API for starting and inspecting runs, and GitLab one for pipelines and the manual `deploy_prd`.
Missing: a service account that can get an Airflow token without the GitLab browser sign-in, and a page listing the
five or six calls that match dbt Cloud's (trigger job, run status, artifacts of a run). How Airflow issues tokens
under the GitLab sign-in has to be checked first.

### 4. Webhooks

1. `airflow/utils/dbt_webhook.py`, modelled on `dbt_failure_report.py`: one JSON POST per DAG run with environment,
   DAG, run id, outcome, `dbt_image_tag` and counts from `run_results.json`, signed with an HMAC header.
2. Airflow Variable `dbt_webhook_urls` (JSON list), off until set and deleted by `make reset`, like the two asset
   Variables.
3. Send it from the DAG's success and failure callbacks, not from an extra `all_done` leaf task (architecture skill
   §7: run state comes from leaf tasks).
4. A receiver for demos: a tiny sink container, or an endpoint in the Workshop Studio as a new evidence type.
5. Tests next to `test_failure_report.py`.

This also closes two rows of `client-gap-report.md`: the "ready" signal to an external scheduler and the call to an
outside system when a data product refreshes.

## Phase 2: new logic, known techniques

### 5. CI "compare changes"

1. A macro `compare_changes` (run-operation) that takes the modified models and compares each CI table in
   `vdp_ci.<branch schema>` with its `dev_*` counterpart: columns added, removed or retyped (information schema), row
   counts, and rows added, removed or changed by key. `svc_ci_dbt` already reads dev through `dev_reader`.
2. The key comes from the model's uniqueness test (`unique` or `dbt_utils.unique_combination_of_columns`); a model
   without one gets counts and columns only.
3. Call it in `verify_feature_to_dev` after the build, before `drop_nodes`; publish the result as a job artifact and
   as a note on the merge request (the GitLab API and a token are already in use for tagging).

Risks: the feeder keeps adding rows to dev, so the comparison needs a cut-off on load time or it reports noise. A
ready-made package (`audit_helper`) is unlikely to run on SingleStore unchanged; plan for an own macro.

### 6. Audit log

Sources that exist: Airflow's event log (who triggered, cleared or changed a Variable), GitLab's project events and
pipelines, `deployed.json` per environment, and the drift audit for changes made outside dbt. Missing: Vault's audit
device (one line in `infrastructure/vault/entrypoint.sh`) and one place to read them together.

Approach: a "Recent activity" section on the overview page that reads the four sources live. Storing them in a
table (`<env>_vdp_mgmt.audit_events`, filled by a small DAG) is the next step when history must outlive the
containers. Limits: GitLab CE has no full audit events, SingleStore's own audit logging is likely not in the
developer edition, and nothing here is tamper-proof.

### 7. Copilot

The substance is an AI coding agent in the devcontainer that knows the project. The engine can install package
skills for an agent (`--ai-provider`, for example `claude`), and the repository already keeps skills in
`.agents/skills/`.

1. **Decide first:** may project code and query results go to an AI provider, and who pays per developer?
2. Add project skills in `dbt-singlestore-workshop/`: the style guide `dbt-bouncer.yml` enforces, how to document
   and test a model, the naming rules.
3. Add the agent to the dev image or as a devcontainer feature, and one `vdp:` task per common request (document
   this model, propose tests).

## Phase 3: trial first, then decide

### 8. State-aware orchestration

**Trial 2:** run two builds in a row with `--state-mode read-write` and see whether the second skips unchanged
models without the dbt platform; then try `--manage-state`. If neither does anything locally, the fallback is:

1. Move `dbt_source_freshness` before the build and keep the previous `sources.json` in the state folder.
2. Build with `--select source_status:fresher+ --state <previous>`, so only tables fed by new data are rebuilt.
3. Keep one full build a week: models that depend on the calendar, not on a feed, go stale otherwise.

The asset Variables already cover the other half: the load starts when its feeds report in.

### 9. Cross-project references

dbt Cloud resolves `ref('other_project', 'model')` through its own metadata service; that part cannot be copied.
A reduced form can:

1. **Governance in the one project** (S, useful by itself): assign the groups of `groups.yml`, set `access`, and
   put contracts on the public models. This also closes two rows of `client-gap-report.md`. Whether the engine
   enforces contracts on SingleStore has to be tried.
2. **Publish**: `publish_artifacts` already stores every version's manifest as a GitLab package.
3. **Consume**: a new `vdp sources import` that reads that manifest and writes the public models of the other
   project as sources, next to `vdp sources add`.
4. **Schedule**: the consumer's load waits for the producer's asset events (`dbt_dataset_dependencies`).
5. A second small dbt project in GitLab to show it. This is the expensive part: bootstrap, CI, image, DAGs and reset
   all grow a second copy.

### 10. Semantic Layer

The engine parses semantic models and metrics (the information schema has their tables) but cannot turn a metric
query into SingleStore SQL, and the serving side (the query APIs) is dbt Cloud only. Options:

| Option | What it takes | Assessment |
|---|---|---|
| A. Add a SingleStore dialect to `dbt-metricflow` in the fork, then export saved queries as tables in the nightly load | Rust work in the fork, kept alive across upstream updates | Only when a consumer needs ad-hoc metric queries |
| B. A separate open-source semantic server in front of SingleStore | A new service and a second place where metrics are defined; its SingleStore support is unverified | Not recommended |
| C. Keep the figures as reports (`mart_olist_management_kpi`) with exposures | Nothing | Recommended for now |

## Not worth building

- **Insights.** The SingleStore web UI on port 8083, `vdp sql run` / `sql export` and `dbt show` cover querying.
  Worth one paragraph in the README, nothing more.
- **Canvas.** No open-source visual editor is known to produce dbt models of this project's shape. Scaffolding
  commands in `vdp` would give most of the benefit at a fraction of the cost.

## For every feature that gets built

- Tests in `dbt-singlestore-workshop/airflow/tests` (`make test`), experiments on a throwaway SingleStore only.
- The matching skill and the README updated in the same change.
- A Workshop Studio scenario where the feature is visible to a non-technical audience (webhooks, compare changes
  and column lineage are; the APIs are not), and its line in `scenarios/_roadmap.yaml`.

## Open questions

1. Does strict static analysis accept this project's SingleStore SQL (trial 1)?
2. Does the engine's state mode do anything without the dbt platform (trial 2)?
3. Can a service account get an Airflow API token under the GitLab sign-in?
4. Is an AI assistant allowed to see project code and data?
5. Is there a second team or project that would make feature 9 more than a demonstration?
