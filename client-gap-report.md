# Client setup vs workshop: logic the workshop does not have

Comparison of 2026-11-22 between the client export (`~/Documents/repos VDAB/`) and this repository. The question: which
logic does the client use that the workshop lacks? This is a report only; nothing in the workshop was changed for it.

**Result:** 33 pieces of logic are absent or only partly present, in Airflow (11), the dbt project (12) and CI (10).
Tooling and process gaps are listed after them.

**How it was made:** six Sonnet agents each audited one area and returned 110 rows in total. About 15 rows were
duplicates between agents. The "absent" rows were re-checked with greps in the workshop; corrections are noted.
The agents' own notes are in the appendix.

**Reading the tables:** client paths in the Airflow and dbt tables are relative to `release-ui/dbt-vdp/` in the
export; paths in the CI table are relative to `jenkins-vdab-dbt/` unless they name another folder.

"Logic" means a mechanism: a macro, a hook, a DAG behaviour, a CI step, a config convention. Business models,
business-rule macros and platform plumbing are not counted (see "Not gaps").

## Airflow (11)

| Client logic | Client file | Workshop today |
|---|---|---|
| Daily load refuses a manual start unless `manual_run_override` is set | `airflow/dags/dbt_daily_load.py:78` | Absent, a manual trigger runs freely |
| After a scheduled build: run the `non_critical_tests` selector and list failed models (`result:error`) | `airflow/dags/dbt_vdp.py:375` | The selector exists, no DAG runs it |
| Failure mail is off for manual runs, with per-run parameters for on/off and recipients | `airflow/dags/dbt_vdp.py:47`, `airflow/utils/dbt_dag_utils.py:339` | Always sent, recipients from the environment only |
| A release remembers whether the daily load was already paused and only unpauses what it paused itself | `airflow/dags/dbt_release_basic.py:153` | `unpause_daily_load` always unpauses (`airflow/dags/dbt_release.py:438`) |
| Ad-hoc DAG takes any dbt command, including `run-operation` with vars or args | `dbt_vdp.py:36`, `airflow/dags/dbt_vdp.py` | `dbt_on_demand` only does a build with select and exclude |
| One generated DAG per data product that calls an outside system when its tables refresh, at most once a day | `airflow/dags/ai_triggers.py:61`, `:98` | Only the notice mail reacts to refreshed tables |
| "Ready" signal to the external scheduler after the daily load | `airflow/utils/dbt_trigger_utils.py:57` | Absent |
| Reload by date window (`window_days`, `batch_vars`, window halved on retry) and a chunk filter in the import CTEs | `airflow/dags/dbt_reload.py:166`, `macros/model_helpers/import_generate_cte.sql:63` | Chunks by parts only, no date windows |
| Real parse test of the DAG folder (`DagBag`) | `airflow/test_dag_parse.py` | Tests use Airflow stand-ins |
| `dbt_release_basic`: a plain list of statements without state merging | `airflow/dags/dbt_release_basic.py` | Absent |
| Upstream models built before every release step | `airflow/dags/dbt_release.py:369` | Only before the first step |

## dbt project (12)

| Client logic | Client file | Workshop today |
|---|---|---|
| Durable reload history: an `on-run-end` hook writes each full rebuild to `dbt_reload_history` | `macros/run_metadata/log_dbt_reloads.sql` | Absent |
| PIT loading history: delete on full refresh, close the cycle afterwards, register only models in this run, cap the batch window | `macros/pit/pre_hook_fmc_pit_loading_history.sql`, `post_hook_pit.sql`, `macros/run_metadata/get_current_run_models.sql` | Only generic open and close of a load cycle (`macros/utils/loading_history.sql`) |
| `grants:` on 591 model configs | `models/**/*.yml` | The sync macro reads grants, but no model or project file sets any |
| Contracts enforced on 43 models, data types on columns | `models/pres_dataproduct/...` | Absent |
| `sql_header` with session tuning on 20 models | `models/edw_dv_bdv/satc/...` | Absent |
| Dev sampling switch (`sample_dev`, `sample_pct`) | `models/edw_dv_bdv/satc/satc_vacature_hb/satc_vacature_hb.sql:128` | Absent |
| Per-environment values in one var, read through `DBT_DEPLOY_TARGET` | `dbt_project.yml:84` | Absent |
| Seeds: a config block and 13 seeds | `dbt_project.yml:57`, `seeds/` | No seeds folder |
| Groups assigned to 103 models | `models/groups.yml` | `groups.yml` exists, no model uses it |
| Reference tables (58 models) and hash indexes (11) | `models/**/*.yml` | Never used |
| `slow_statement_threshold` on every target | `profiles.yml:9` | Absent |
| Reconciliation tests comparing staging with target, with a `meta` header | `tests/assert_*.sql` | Four singular tests, not this pattern |

Correction on physical design: the agent reported one workshop model with a shard key. A grep finds shard, sort and
primary keys on a handful of workshop models, so only reference tables and indexes are missing.

## CI and release (10)

| Client logic | Client file | Workshop today |
|---|---|---|
| One build at a time | `jenkins-vdab-common/vars/vdabPipeline.groovy` | No `resource_group`; two pushes to dev can race on the tag |
| Git tag set only after image and artifact succeeded | `vars/dbtBuild.groovy:46` | The tag stage runs before the build (`stages: [verify, tag, build, deploy]`) |
| Weekday promotion to acceptance on a clock | `vars/dbtPipeline.groovy:35` | No schedule anywhere |
| Production gets what the previous environment runs now (version read from the image label) | `vars/dbtPromote.groovy:29` | `deploy_prd` only needs `deploy_val` of the same pipeline |
| Release number must come from the merge message, otherwise the build fails | `src/dbt/Versioning.groovy:31` | `tag_main` falls back silently to the latest tag or Q1 |
| Emergency drop lane, patch flow, numbering floor, year rollover | `src/dbt/Versioning.groovy:165` | Only `YYYY-Qx.N` tags |
| Publish failure is fatal; docs pin and an unreachable Airflow server only mark the build unstable | `vars/dbtDocsPin.groovy:22`, `vars/dbtAirflowSync.groovy:101` | Inverted: `publish_artifacts` may fail, docs generation is fatal |
| Pipeline rules tested on every push | `test/` | No tests for the tag and routing rules |
| Artifact compiled per environment, so names match that environment | `src/dbt/Images.groovy:43` | Compiled once as dev |
| CI secrets read from the vault per tier | `src/dbt/PipelineSecrets.groovy:16` | Literals in the CI file; no job calls the Vault stand-in |

## Process and tooling

- **Jira as the promotion contract.** The fix version decides keep, revert or emergency, and the ticket key is
  enforced in branch names and merge messages. The stack has no Jira stand-in, so `vdp release analyse` has nothing
  to talk to. The Studio roadmap already lists "one screen to prepare a release" as a next step.
- **Developer toolkit.** Compared with the client's Python helpers (`dbt-singlestore-base-image/vdp-python-utils`),
  the Rust `vdp` lacks the Excel drop report, a Jira search by fix version, the cherry-pick release candidate, the
  maintenance JSON, Vault writes and run log files.
- **Release hub.** Only the client copy (`release-ui/crates/vdp-release-hub`) has credentials saved from the UI for
  Jira and SMTP (`src/integrations.rs`, `PUT /credentials/<service>`), the take-over of the starting user's git
  identity, the Svelte frontend (`release-ui/frontend`) and its own Dockerfile, compose file and Makefile. The
  VaultSpeed hub crate sits in `release-ui/crates` at the client and in the `vaultspeed-framework` workspace here.
- **VaultSpeed.** The client has an agent that turns a generated zip into a release branch
  (`vaultspeed-automation-framework/vaultspeed_agent.bash`), a CI chain and upstream load DAGs. The workshop's
  deployer is standalone, and `vdp vaultspeed` exits with an error in the dev image.

## Not gaps

- **Done differently on purpose:**
  - one image for all environments instead of one per environment;
  - docs generated at deploy instead of a pinned docs image with a GitOps repository;
  - maintenance at release only, which the roadmap names as the client's next step (the client also runs it at
    deploy and during the image build);
  - an Airflow Variable for the running version instead of registry tags and digest promotion;
  - pausing DAGs through the Airflow database instead of its REST API.
- **Already decided as next steps:** the `should_full_refresh` override in place of the per-table PIT switches, and
  the generated docs front page.
- **Domain content (8 rows):** hashing, timeline, hub key and text macros, the incremental dependency framework and
  the pseudonymisation hash.
- **Platform plumbing:** TLS, proxy, image signing, hardened containers, lint and secret scans of images, retention,
  Kubernetes settings, the Airflow 2 and 3 shim, private package access.

## Unconfirmed

- **Which client DAGs are live.** Jenkins syncs only `airflow/dags/*` (plus `airflow_local_settings.py` and
  `compat.py`), so those copies of `dbt_vdp.py` and `dbt_release.py` are the deployed ones and the top-level copies
  in `dbt-vdp/` are not. How `airflow/utils` reaches the Airflow server was not confirmed.
- **Environment-gated models.** The workshop has one. Whether orphan clean-up spares models that are gated out of an
  environment was not tested.
- **Rows taken from an agent without a re-check of mine:** the client-side counts in `models/` (591 grants, 43
  contracts and so on) and the comparison of the Python helpers with the Rust toolkit.

## Appendix: the agents' notes per area

### Macros, tests, seeds

#### gaps-macros notes (client = release-ui/dbt-vdp, workshop = dbt-singlestore-workshop)

1. log_dbt_reloads (macros/run_metadata/log_dbt_reloads.sql:1-87) + dbt_project.yml:65 hook: after every run on target `run` (or var log_reloads) inserts one row per incremental model that was fully rebuilt into model dbt_reload_history (invocation, timing, rows, strategy, chunks, verification, target, user, dbt version). Looks up relation through the graph, skips with log message if table missing. Workshop: grep log_dbt_reloads|dbt_reload_history|reload_history in macros, models, airflow, infrastructure/airflow: no hits. Workshop keeps in-flight state (reload_state_table in reload_helpers.sql) but no durable history of reloads per dbt run.
2. PIT loading-history lifecycle: pre_hook_fmc_pit_loading_history.sql:1-54 deletes history rows of a PIT when it is fully refreshed (should_execute guarded); post_hook_pit.sql:1-15 commits (cross-database workaround) and closes the newest open cycle by max(dv_load_cycle_id); get_union_of_pit_run_models.sql + get_current_run_models.sql (uses selected_resources) generate history rows only for PIT models in this run; var max_nr_days_batch_pit caps window (dbt_project.yml:87, fmc_pit_loading_history.sql:78). Workshop: macros/utils/loading_history.sql has open_load_cycle/close_load_cycle (generic, skip on ci/reload, guarded) used by dummy_reload only. grep selected_resources, get_current_run_models, max_nr_days_batch: none. No delete-on-full-refresh, no run-selection-driven registration, no batch cap, no commit step.
3. Incremental dependency framework: incremental_setup_dependencies.sql, _normalize_dependencies, _date_range_build, _date_range_join_satellites; three modes time_delta_active_from_source / time_delta_active_from_target / changed_entity_full_rebuild, incr_start/incr_end from env INCR_LOAD_START/END. Workshop: only get_incr_load_start/end (variables/get_incremental_window.sql, identical); grep time_delta|changed_entity|incremental_normalize: none.
4. chunk_column / chunk_where in import_dependencies (model_helpers/import_dependencies.sql:1, import_generate_cte.sql:63): per-chunk filter on every import CTE during a chunked rebuild (dbt fork macro chunk_where, config chunks). Workshop import_generate_cte has materialize_off and respect_empty but no chunk_column; grep chunk_where: none. Workshop chunks in reload_helpers via Airflow batches instead.
5. Data vault hashing: generate_md5_hash.sql (null handling, delimiter escaping, binary(16), timestamp/binary formatting), build_lagged_hash_diff_cte.sql (lag hash_diff, keep only changed rows, recompute valid-to). Workshop grep unhex(|hash_diff: none.
6. generate_ikl_hash.sql: sha1('IKL-'||col||salt from env IKL_HASH_SALT). Workshop only scrubs the salt from compiled_code in parse_dbt_results.sql (same as client); no hashing macro.
7. Timeline macros: generate_x_geldig_tot_dt (lead over partition), generate_date_range, generate_date_range_join_condition, generate_null_if_deleted, get_most_recent_cte, generate_x_datum_dakey, generate_x_cre_fields (x_cre_dt, batch id = invocation_id). Workshop grep x_geldig|lead(: none.
8. generate_hubc + split_bk_col: reads hub table columns, finds *_bk, splits to bkcc/id|uuid with lengths from vars id_column_length/uuid_column_length/bkcc_column_length; raises compiler error on bad type; EMPTY aware. Workshop grep _bk: none.
9. Text/date utils: strip_html, url_decode_common, format_sentence_case, add_time_to_date, get_first_day_of_current_month, generate_datetime. Workshop grep regexp_replace: none.
10. table_exists.sql: adapter.get_relation wrapper. Workshop has load_relation use and adapter.get_relation in drop_old_relations; no named helper.
11. generate_database_name: client uses custom database as is, `ci` target = target.database; workshop prefixes <env>_ and ci = vdp_ci (differs on purpose). generate_schema_name collibra: client uses node.database, workshop calls generate_database_name (Fusion reason). generate_alias_name: workshop adds __new_ prefix for reload.
12. tests/integration/test_dbt_vdp_k8s.py: pytest DagBag: DAG loads without import errors, and dag.test(run_conf command=compile, mark_success_pattern for a trigger task, connections/variables json). Workshop: dbt-singlestore-workshop/airflow/tests has test_dag_wiring, test_release_steps etc, no DagBag dag.test() run of a DAG end to end (grep DagBag|dag.test|import_errors: none).
13. tests/assert_*.sql (22 files): shared pattern: config meta Test/Doel/Creatie, severity warn, import_dependencies of target and staging, compare counts/keys (staging vs target reconciliation). Workshop tests/carrier, tests/web: 4 singular tests with severity/tags, no meta header, no import_dependencies reconciliation pattern.
14. seeds/meta: only .gitkeep, nothing to compare. Workshop has no seeds dir.
15. tests/setup_tests.sh: present both (workshop adds DBT_DEPLOY_TARGET).

Present both: should_execute, create_database no-op, generate_* naming trio, drop_nodes (workshop better), cleanup_user_models (workshop restricts to dev schemas), perform_database_maintenance/check_database_maintenance (identical), log_dbt_results/parse_dbt_results (workshop env-prefixed mgmt db), get_incr_load_*, get_local_run_started_at, star (workshop falls back to *), get_column_names, import_dependencies, import_generate_cte (minus chunk), setup_tests.sh.
Workshop-only (not gaps): should_full_refresh, should_log_results, loading_history, reload_helpers, drift audit, sync_grants.

### Project config and model conventions

#### Gaps: dbt project config and model conventions (client vs workshop)

Client = release-ui/dbt-vdp. Counts are from grep over models/ (1921 sql, 1691 yml).

1. Per-model SingleStore physical design in yml config: shard_key 551, sort_key 398, primary_key 320, reference:true 58, indexes (hash, CDC index on x_bron_upd_dt) 11, sql `partition_by=` 247 (window macro arg, not a config key).
   Workshop: models/ has shard_key 1, primary_key 2 (both dbt_run_results), no sort_key/reference/indexes on business models.
2. Model-level grants: config.grants { select: ["role ..."] } on 591 yml configs (roles im_monitoring_ro, im_planning_ro, dp_im_*_ro). Workshop has macro sync_grants_with_singlestore reading node.config.grants, but grep 'grants' in models/ finds no model that sets it. Convention not exercised.
3. groups.yml + `group:` on 103 models (client groups.yml:1-). Workshop has groups.yml but grep `group:` in models/ = 0 assigned. No `access:` anywhere on either side (0 client).
4. Contracts + column data_type: contract.enforced true on 43 models (1 false), column constraints (not_null) 9, `data_type:` 5441 column lines. Workshop: 0 contract, 0 data_type.
5. sql_header with session variables (materialize_ctes off, optimizer_disable_right_join ON, enable_broadcast_left_join off, singlebox_optimizer_cost_based_threshold=30): 20 models. Workshop: grep sql_header/'set session' = 0.
6. on-run-end log_dbt_reloads(results) under `target.name == 'run' or var('log_reloads', false)`, writes vdp_mgmt.dbt_reload_history (model with full_refresh:false, incremental append). dbt_project.yml:42-43, models/vdp_mgmt/dbt_reload_history. Workshop on-run-end has only log_dbt_results; grep log_dbt_reloads / dbt_reload_history / log_reloads over repo (excl. src, target) = no hit.
7. Chunked/swap full refresh in model config (full_refresh_strategy: swap + chunks: 8/16 on 10 models) is the client side of the known reload_strategy; not reported. dbt_reload_history columns full_refresh_strategy/chunks/chunk_verification are the logging half (see 6).
8. PIT loading framework: per-PIT full_refresh via check_full_refresh_pit_*() macros bound to vars full_refresh_pit_*_db (dbt_project.yml:70-74), var max_nr_days_batch_pit, model edw_fmc.fmc_pit_loading_history (incremental delete+insert) with pre_hook_fmc_pit_loading_history and post_hook_pit on 4 PIT models, daily_snapshot_dates. Workshop has dummy_reload_pit_with_deps and loading_history.sql (open/close load cycle), but no PIT vars/hook/history model. Macro bodies belong to the macro agent.
9. Per-environment date cutoff vars: dict vars keyed LDV/ABT/CBT/PRD (bit_cutoff_date_2026_dt 17 uses, start_organisatie_label_v3_2026_dt, start_correctie_*, start_brgdvl_*), looked up as var(x)[env_var('DBT_DEPLOY_TARGET')], dbt_project.yml:84-120. Workshop vars: local_run_started_at + drift_audit_*; no env-keyed dict.
10. Column-length vars id_column_length/uuid_column_length/bkcc_column_length (used 4/3/3). Workshop: absent.
11. Env-conditional enabled: `enabled: "{{ env_var('DBT_DEPLOY_TARGET','LDV') in [...] }}"` (3 models) + 30 enabled:false. Workshop: 1 model with enabled=env_var(...) != 'dev' (stg_olist_orders); same mechanism, small use: done.
12. Dev sampling: var sample_dev / sample_pct (+ target.name=='dev') crc32(hkey)%100 filter, e.g. satc_vacature_hb.sql:128-131. Workshop: grep sample = 0.
13. Seeds: seeds: block with +database and +delimiter, 13 seed csv+yml (seeds/edw_dv_bdv). Workshop: no seeds dir, no seeds: block (selectors still reference seeds).
14. Layered folder-to-database config: 7 layers with +database/+tags and node_color: edw_dv_bdv (+c2_conversie sub-tag), pres_dataproduct, im_dimensional, edw_ldm, edw_fmc, vdp_mgmt, tenant_ai (dbt_project.yml:20-52); sources for 14 databases (psa, vdp_psa, edw_inc, edw_mtd, edw_dv_rdv, tenant_*). Workshop: 2 layers (edw_dv_bdv, im_dimensional); vdp_mgmt database is set per model (database: vdp_mgmt in model config) rather than as a folder default. Domain/org mostly; the cross-layer tenant_ai consumer view vdp_dbt_run_results (filters run results on database) is org plumbing.
15. Tags used operationally: on-demand (4 models, excluded in Airflow commands and release), boin (28), crm, burger, forced_refresh, monthly. Workshop selectors daily_load/daily_load_monthly exclude on-demand/monthly, and tags exist on tests (critical); grep tags in workshop models = 1. Mechanism present, little exercised.
16. Source freshness: client 0 freshness/loaded_at_field (checked sources/*). Workshop has 3 freshness + 6 loaded_at_field: workshop is ahead. Not a gap.
17. Model docs: doc() blocks (3 md files, 4 uses). Workshop 0 doc blocks. Minor.
18. Singular tests with severity warn (20 tests in tests/ with config(severity="warn")); `where:` on 61 generic tests. Workshop: tests/ has 4 singular tests, `where` used. Present.
19. Selectors: client has 3 (critical_tests, non_critical_tests, daily_load); workshop has 4 (adds daily_load_monthly, tag critical). Workshop superset. Client daily_load description says excludes monthly but definition only excludes on-demand (client oversight, not reported).
20. Profiles: client has default/ci/run/collibra with ssl_mode required, allow_cleartext_plugin, slow_statement_threshold 300 (all targets); workshop has default/ci/run/reload/collibra without those (documented: no TLS in workshop). slow_statement_threshold is an adapter option the workshop omits.
21. Client clean-targets uses the same 'pu' user-prefix expression; present both sides.
22. Not found on either side: exposures in client (0), unit_tests (0 both), snapshots (none), versions (0), persist_docs (0), access (0), semantic models (0). Workshop has models/exposures.yml (6), client none.
23. dbt-bouncer: same four checks; workshop has the str_to_date regexp fixed. packages: same dbt_utils >=1.3.3 (client lock 1.4.1). vdp.toml: workshop-only (Vault, sources prefix); client has none in the dbt project root.
24. vdp_mgmt models: client 12 (dbt_active_test_failures, dbt_foreign_create_user, dbt_model_info, dbt_models, dbt_orphaned_objects, dbt_reload_history, dbt_run_results, dbt_sources, dbt_table_grants, dbt_table_performance, singlestore_column_info, singlestore_table_info). Workshop has all except dbt_reload_history.

### Airflow

#### Airflow gaps: client vs workshop (notes)

Paths: C = client release-ui/dbt-vdp/airflow (and top level), W = dbt-singlestore-workshop/airflow.

Key structural finding: client top-level dbt_release.py / dbt_vdp.py are the NEWER utils-based Airflow-3 versions (workshop dbt_release is
nearly a copy of the top-level one). airflow/dags/dbt_release.py and dbt_vdp.py are the OLDER inline kubernetes-pod versions (Airflow 2 style imports, compat.py).
utils/dbt_pod_utils.py and tests/test_pod_utils.py are byte-identical on both sides (diff = 0).

1. ai_triggers.py (C dags/ai_triggers.py:76-139): one DAG per entry of Variable AI_KOW_dataset_mapping, scheduled on the Assets of that entry (single or list),
   tasks skip_if_already_run_today (prev_*_success, Europe/Brussels date compare, AirflowSkipException) >> notify_ai (POST to Variable ai_trigger_uri). W: grep AI_KOW/ai_kow/prev_ none; only report_refreshed_notice consumes assets (known).
2. skip_if_already_run_today guard (C ai_triggers.py:98-119): once-per-local-day guard for asset-triggered DAGs. W: no equivalent (grep prev_, already_run).
3. dbt_release_basic.py (C 1-239): statement-list release, no upstream prep / no state merge / no final build; none_failed trigger so skipped (dry-run) statements still log; release_finished leaf; dry_run.
   W: no such DAG.
4. pause restore (C dbt_release_basic.py:153-222): reads paused state of dbt_daily_load first (XCom WAS_PAUSED), fails if unreadable, unpauses only if this run paused it. W dbt_release unpauses unconditionally (dbt_release.py:438-445); grep was_paused none.
5. dbt_daily_load manual-trigger gate (C dags/dbt_daily_load.py:78-81, utils/dbt_dag_utils.py:291-316): validate_trigger_source blocks manual runs unless param manual_run_override; scheduled/asset_triggered pass. W: grep validate_daily_run_trigger/manual_run_override none.
6. Cronacle ready signal (C utils/dbt_trigger_utils.py:57-101, dbt_daily_load.py:144, dbt_vdp old :492-526): SOAP POST after the build and BEFORE asset events; credentials from Variables; fails loudly if unconfigured. W: grep cronacle none (org plumbing).
7. Old dbt_vdp non-critical tests (C dags/dbt_vdp.py:406-442): after the signal, scheduled runs execute `dbt test --selector non_critical_tests`; manual runs only list. W: selector exists (selectors.yml:25) but no Airflow task runs it (grep non_critical in W none; only selectors.yml).
8. Ad-hoc dbt_vdp (C top-level dbt_vdp.py, utils/dbt_command_utils.py:181-224): free-form command (build/run/test/run-operation...), vars (--vars vs --args by operation), exclude merged with Variable disabled_dbt_models, threads Variable, update_datasets branch (BranchPython) to emit outlets, per-run params send_email_on_failure and failure_email_recipients.
   W dbt_on_demand: select-based `dbt build` only (build_on_demand_command), no free command, no run-operation, no vars param (only full_refresh_models), no per-run mail toggle.
9. Per-run failure mail controls (C utils/dbt_dag_utils.py:339-405): params send_email_on_failure / failure_email_recipients override, env default DBT_FAILURE_EMAIL_RECIPIENTS; old dbt_vdp:47-68 suppresses mail for run_type manual. W dbt_failure_report.py:623 env-only recipients, report always sent (failure_report_task one_failed).
10. dbt_reload date windows (C dags/dbt_reload.py:166-268, utils/dbt_command_utils.py:369-502): window_days param (fixed-size windows), computed in Airflow from start/end/batch_count, INCR_LOAD_START/END env export per chunk, free-form batch_vars, halving of numeric var / date window on retry.
   W: reload_batch macro with batch/batches/parts (parts doubles on retry), params start_date/end_date/limit; no window_days, no batch_vars (grep window_days, batch_vars none). INCR_LOAD_* exists only as dbt macro env vars.
11. Old dbt_vdp `list_failed_models` (C dags/dbt_vdp.py:375-404): `dbt list --select result:error --state artifacts` after archive, lists failed models in the log. W: grep list_failed, result:error none (failure report parses run_results instead).
12. Old dbt_vdp run_type-forced command (C dags/dbt_vdp.py:229-234): scheduled/dataset/asset-triggered always `build --selector daily_load`, params ignored. W: daily load has no params, fixed command; N/A mostly.
13. Pause via REST (C utils/dbt_dag_utils.py:29-288): GET/PATCH /api/v2/dags/{id} with JWT from connection token, Vault secret or login; TLS verify settings; tests in tests/test_dag_utils.py (262 lines). W set_dag_paused_state (utils/dbt_dag_utils.py:22-89) = ORM DagModel then `airflow dags pause` CLI with env read from /proc/1/environ. Done differently; no get_dag_paused_state; no tests (grep set_dag_paused_state in W tests none).
14. airflow_local_settings.py (C 1-57): PodDefaults.SIDECAR_CONTAINER (xcom sidecar image = dbt image, 10m cpu / 32Mi), InsecureRequestWarning filter, pod_manager log filter. W infrastructure/airflow: only Dockerfile/entrypoint.sh/webserver_config.py; grep PodDefaults none. K8s only.
15. compat.py (C 1-46): Airflow 2.x/3.x shim (Dataset/Asset, Param, task, exceptions, EmptyOperator, BranchPython). Used only by old dbt_vdp. W: Airflow 3 only, grep compat none.
16. test_dag_parse.py (C 1-32): real DagBag parse of airflow/dags, asserts no import_errors, dbt_vdp present with tasks. W: tests use stand-ins (test_dag_wiring.py), grep DagBag/import_errors none in W tests/Makefile/manager.
17. Old dbt_release per-step upstream (C dags/dbt_release.py:369-421): every `-s model` step gets build_upstream__ task (`+model` exclude model tag:on-demand [result:success with --state]). W: upstream prep only for first step (dbt list, dbt_release.py:176-186).
18. Old dbt_vdp clean_artifacts + archive_artifacts_to_pvc (C :280-308): rm artifacts, cp target -> artifacts, all_done. W: persist_artifacts_to_state does cp only (equivalent enough).
19. reload archive XCom (C dbt_reload.py:318-329, dbt_state_utils.py:153-161): do_xcom_push summary {status, model, path}, warning when run_results missing. W consolidate_reload_artifacts, no XCom.
20. Dataset outlets for ALL successful table/view/incremental models + filter by ai_kow mapping (C dbt_vdp old :449-476, dbt_state_utils.py:96-150). W: known opt-in asset events; skipped.
21. top-level vs airflow/dags: see structural note above.

Present on both: pod utils (identical), dbt_state_utils merge, release step expansion w/ accumulated state, reload delegation w/ TriggerDagRun 5h timeout, retry via `dbt retry` when try_number>1, max_active_runs=1, failure mail, daily-load schedule on assets.

### CI and release

#### CI / versioning / packaging / promotion / deploy: gaps (client vs workshop)

Paths: C = client export, jenkins-vdab-dbt (lib), jenkins-vdab-common (common); W = dbt-singlestore-workshop/.gitlab-ci.yml.

1. Deploy-time database maintenance stage. C: vars/dbtMaintenance.groovy:22-32 runs perform_database_maintenance in a dbt-vdp image as the TARGET env (DBT_DEPLOY_TARGET of target, credentials of target) after publish, before DAG sync; dbtBuild.groovy:69, dbtPromote.groovy:60. Failure blocks DAG sync. W: grep 'maintenance' in .gitlab-ci.yml = no hit; only the Airflow dbt_release DAG does it (client has that too, docs/maintenance.md:41).
2. Image per environment. C: Images.build (src/dbt/Images.groovy:27) builds dbt image + docs image per env with DBT_DEPLOY_TARGET build-arg, BuildKit secrets (ssh key, approle, db creds; Commands.groovy:101-113), --pull --no-cache. W: Dockerfile builds one image for all envs, target set at runtime, no secrets in build, cached layers.
3. Artifact tarball compiled per environment. C: target.tar.gz is produced inside the env image (docker cp, Images.groovy:43-56) so names match the env database. W: publish_artifacts (.gitlab-ci.yml:179-195) compiles once with DBT_DEPLOY_TARGET=dev for every version; comment lines 175-178 say so. Partial.
4. Registry push + floating env tags + promotion by tag move. C: Images.tagAndPush (Images.groovy:59-70) pushes version tag, semver twin, floating tags (ldv+latest, abt, cbt, prd; Environment.groovy:37-47); Images.promote (:79-97) compares digests, moves tag, no-op when equal; Artifacts.promote (Artifacts.groovy:33-45) compares checksum, replaces. W: images stay on host daemon (no push, .gitlab-ci.yml:162-167); env->version is the Airflow Variable dbt_image_tag; deploy_prd only `needs: deploy_val` of the SAME pipeline (:260-267), nothing checks prd gets what val currently runs; no digest-equal skip.
5. Airflow sync to several servers with reachability probe. C: vars/dbtAirflowSync.groovy:75-113: host names from Vault, missing key = hard fail naming key; ssh probe 15 s; unreachable server skipped, build UNSTABLE; none reachable = fail; rsync dags + airflow_local_settings.py + compat.py. W: .gitlab-ci.yml:209-222 copies dags and utils into a volume and docker exec's one container; grep airflow_local_settings|compat.py outside src = none. Single target, no skip-and-flag.
6. Docs pin (GitOps). C: vars/dbtDocsPin.groovy:17-37 + resources/dbt/pin-docs-tag.sh: check docs image with that tag exists, reset env branch to upstream branch, check values file exists, set global.tag, commit only if changed, push --force-with-lease, failure = UNSTABLE not FAILED. Docs are a separate nginx image built per release with docs generated against that env, promoted with the env tag. W: docs generated at deploy into the state volume (.gitlab-ci.yml:215-221), step fatal if docs generate fails; no pin repo, no image existence pre-check, no unstable level.
7. Secrets read at run time per tier. C: src/dbt/PipelineSecrets.groovy:16-100 + Environment.groovy:13 (nonprod->dev vault cluster, prod->prd), approle, airflow ssh key, DB creds per env, passed into containers by variable name only (Commands.groovy:55-67). W: .gitlab-ci.yml:14-16, 206 literal fixture passwords; grep vault in .gitlab-ci.yml = none (vault container exists but CI does not call it).
8. Image signing. C: Images.groovy:70-73 -> common vars/vdabDocker.groovy:260 cosign by digest after push. W: grep cosign|sign = none.
9. Concurrency lock. C: common vars/vdabPipeline.groovy: disableConcurrentBuilds default true (also called out in docs "Welke flow start wanneer"). W: grep resource_group|interruptible in .gitlab-ci.yml = none; two pushes to dev can race on tag_dev (latest tag read then pushed).
10. Scheduled promotion. C: vars/dbtPipeline.groovy:35 cron '0 9 * * 1-5' job daily-abt-sync ldv -> abt (Router.groovy:16). W: grep schedule/cron in .gitlab-ci.yml, manager, gitlab = none (only a runner-scheduling comment). No pipeline schedule.
11. On-demand first prod-tier build. C: cbt build only via named job deploy-to-cbt (Router.groovy:16-20), prd via deploy-to-prd (promote of what cbt runs). W: val deploys automatically on tag from push to main (:253-258); only prd is a manual job. Approval gate on the first release environment is absent.
12. Release name must come from merge commit. C: Versioning.nextVersion (Versioning.groovy:31-41) throws when no release/ name in message. W: tag_main (:145-150) silently falls back to latest tag or current year Q1.
13. Release numbering rules. C: previousReleaseTag/nextReleaseTag (Versioning.groovy:165-207): monotonic across branches ("never go below what is out"), new year restarts, leading zero kept, emergency drop release/<y>_EDROP<n> = patch of latest release, never its own tag; fails if no release to patch. W: tag_main numbers per release name only (:151-157); verify/tag regexes only know release/YYYY-Qx; grep EDROP = none.
14. Semver twin tag. C: Versioning.semverTag (:62-72), Environment.imageTags (:119), dbtBuild.groovy:64 second git tag on same commit + extra image tags. W: absent (workshop tags are already single-form). Needed only for OpenShift.
15. Order of tag vs build. C: git tag is set only after both images built, pushed and artifact published (dbtBuild.groovy:46-67); failed build leaves no tag; rerun takes same version. W: tag stage runs first (:112-160), build_image after; failed build leaves a tag whose image does not exist.
16. Hardened tool containers. C: withDbtEnv.groovy:46 + dbtPromote/dbtDocsPin: getHardenedArgs (no-new-privileges, cap-drop all) and --pull=always on dbt/tools containers. W: image default user root (:3-7), host docker socket mounted for jobs.
17. Pipeline library tested on every push. C: Jenkinsfile:4-15 gradle test (rules specs, flow specs, a spec running pin-docs-tag.sh against real git repos; docs/testing.md). W: grep tag_dev|tag_main|gitlab-ci in tests = none; tag and routing rules are inline bash/YAML and untested.
18. Private package access in verify. C: vars/dbtVerifyFeature.groovy:51-57 (ssh key to fetch git dbt deps) + writes .devcontainer/.env for dbt_wrapper. W: .deps snippet only, public packages.
19. Retention. C: common vdabPipeline buildDiscarder 10 builds / 10 artifacts. W: no expire_in, no package cleanup, local images accumulate.
20. Failure severity map. C: artifact publish and maintenance fatal; docs pin and unreachable Airflow server UNSTABLE. W: publish_artifacts allow_failure (:182), env-artifact upload warning-only (:241), docs step fatal. Inverted/weaker for publish.

Checked and not gaps: rollback (workshop richer), two-round verify (client single round, workshop extra), verify_release (client deliberately has none), artifact versioned+env upload (both), env overwrite in place (known).
Client verify = baseline compile from merge-base; workshop merges onto origin/dev: done differently, not a gap.
Client default semver bump is minor (docs/known-issues.md:43), workshop default is patch: rule difference, not logged as a gap.

### Images, developer tooling, docs site

#### gaps-images notes

Client = base-image dir (Dockerfile, v2/openshift/dbt/developer Dockerfiles, Jenkinsfile), vdp-python-utils/, release-ui/dbt-vdp (dev/docs Dockerfile, entrypoint), vdp-dbt-docs (helm).

1. get_db_maintenance_json.py:1-39 runs `dbt --quiet run-operation check_database_maintenance --target run`, parses the last line ending `}}` as JSON, writes maintenance.json. Workshop: macro check_database_maintenance exists (macros/utils/utils.yml:20) but grep of vdp crates for it: 0 hits -> no command / no JSON output.
2. refresh_dbt_metadata_seeds.py:1-147 lists models/seeds/snapshots and sources via `dbt list --output json`, writes seeds/meta/dbt_models.csv+yml, dbt_sources.csv+yml (vdp_mgmt seeds). Workshop: grep dbt_models|seeds/meta in vdp, vaultspeed, vdp.toml, scripts, seeds, macros: 0 hits.
3. update_airflow.py + airflow_manager.py:182-210 sync_project: over SSH (paramiko) git fetch --tags, reset --hard <tag>, hard-link dags to dags dir. Workshop deploys by image tag + DAGs copied into state volume in CI (.gitlab-ci.yml deploy); vaultspeed-framework uploads DAGs over SFTP (vdp-ssh) but only for generated DAGs. No vdp command.
4. create_release_candidate.py:1-239: since a date, cherry-pick (-x -Xtheirs, skip empty) dev commits whose Jira fixversion matches onto release/candidate reset from default branch. Workshop: `vdp release create` is revert-based only; grep "candidate" only hits qa view-candidates. Probably superseded by create_release.py, but the cherry-pick-forward mode is gone.
5. analyse_ticket_status_merged_branches.py:1-552: 8-sheet styled Excel (summary, in scope, excluded, no fixversion, other drops, reverts on base, DROP Jira, missing). Workshop `vdp release analyse` prints/JSON; grep xlsx|excel in vdp-rust-utils, release-ui: 0. Jira JQL list of tickets per fixversion (jira_manager.py:138, component DBT, issue types) not in vdp-jira (only rest/api/2/issue and transitions) so "missing: in Jira for the DROP but not merged" is absent.
6. generate_diff_csv.py:1-144: compares two CSVs (dbt_model_info_<env>.csv) on schema.table.column and writes differences (env drift on column level). Workshop: grep dbt_model_info|diff_csv: 0 (workshop has a drift audit in Airflow/manager, other mechanism, not in my area).
7. vaultspeed_manager.py (1465 lines) + deploy.py: PML/DDL/ETL deploy, add_email_notifications, add_pool, add_dataset_outlets, manual changes rewrite, archive zip. Workshop: vaultspeed-framework workspace has equivalents (email, pool, manual_changes), `vdp vaultspeed` hand-off, but not built in any image (dev image Dockerfile builds only vdp-cli) -> `vdp vaultspeed` exits 2 in the image.
8. dev.Dockerfile:20-36 bakes into a per-environment image (ARG DBT_DEPLOY_TARGET, ENVIRONMENT in docs.Dockerfile:2) at build time with BuildKit secrets (approle, db host/user/pass): dbt deps, compile --target collibra, docs generate --static, perform_database_maintenance, tar target, delete dbt.log. Workshop Dockerfile: only `dbt deps` with retries; compile/docs in separate CI job (.gitlab-ci.yml:175-193) and deploy job writes docs to /state/docs; one image for all envs, environment at runtime.
9. docs.Dockerfile:1-23: separate nginx image per environment (copies index.html, catalog.json, manifest.json from CI image to docroot, port 8080). Workshop: nginx.conf serves /state/docs dirs per env on 8081/8082/8086; no docs image, no per-release artifact.
10. vdp-dbt-docs/ (helm chart vdab-helm-angular 2.10.5 pinned, values.yaml global.tag pinned (docs image version), per-env values files, GitOps branch per env, renovate.json, tiny resource limits). Workshop: no versioned/pinned docs deploy; deployed version is in deployed.json / front page. Org plumbing mostly.
11. vdp-python-utils HashiCorpVaultManager (keyvault_manager.py:126-176): set/delete/list secrets. Workshop vdp-secrets: read only (grep set_secret|kv put: 0).
12. LogManager (log_manager.py:92-155): run log file per project with timestamp, save_logs_to_json, rich console. Workshop: no equivalent per-run JSON log (vdp prints; wrapper reads dbt.log for hint only).
13. ConnectionManager pre_run_hooks/post_run_hooks + query history (connection_manager.py:60-127): SQL run at session start/end, history of queries. Workshop vdp-db: grep: 0.
14. call_sql.py:46-59 interactive prompt to save CSV; workshop `vdp sql run --csv-dir` (non-interactive). done differently.
15. dbt_wrapper: --args/--vars YAML repair, selector guard: present (vdp-dbt). Alias `dbt` in .bashrc -> python wrapper; workshop `dbt` symlink to vdp. done differently/better.
16. docker-entrypoint.sh: `umask 000` then bash (shared files group writable). Workshop: no entrypoint; chmod a+rwX in deploy.
17. bashrc: git-branch/status prompt (devcontainers theme), terminal title. Workshop: no bashrc grep hits in Dockerfiles/devcontainer.
18. Base: MySQL client plugin rpm + gpgcheck=0 + LIBMYSQL_ENABLE_CLEARTEXT_PLUGIN + mysqlclient (SQLAlchemy). Workshop: ADBC only. dbt build: client builds Fusion (dbt-fusion submodule, `cargo build -p dbt-sa-cli`) and Go ADBC driver (CGO c-shared, tags driverlib); workshop equivalent with jimvekemans fork; client includes python dbt-singlestore adapter, sqlfmt, dbt-bouncer copies, requirements with black, pre_commit, duckdb, polars, hvac, GitPython, jinja2-simple-tags.
19. Jenkinsfile: manual version var, `--no-cache --pull`, ssh-key secret, push version + latest + unique version only on main. Workshop: make targets. Org plumbing.
20. Org plumbing: UBI/openshift variants, internal base image & artifact registry, CA certs vdab_root_intermediate_{nonprod,prod} (workshop: ca_certificate "" per env, vdp supports it), /airflow dir 775 openshift fix, microdnf.
Present both: sqlfmt fork + dbt-bouncer + pre-commit (workshop .pre-commit-config.yaml: sqlfmt, dbt parse, dbt-bouncer), sources add/rewrite, release create/analyse/patch/promote/branches, jira advance, sql run/export, qa orphan-tables/view-candidates, env show/doctor, Vault read with AppRole, CA per env, dbt wrapper guard, collibra compile target, target.tar.gz, safe.directory, sudo dbt user.

### Architecture checklist and VaultSpeed

#### Gaps: client architecture overview checklist (data/*.ts) + VaultSpeed automation

Client files cited relative to vdp-architecture-docs/src/lib/data/ (A) and vaultspeed-automation-framework (B).
Workshop checked: both skills, README, _roadmap.yaml, greps in dbt-singlestore-workshop/ and infrastructure/ (excl. src, target, base-image).

##### Part A checklist (P = present, D = done differently, PA = partial, A = absent)
| # | capability (client) | file:line | status | check |
|---|---|---|---|---|
| 1 | Jira INTEL-* ticket as sole promotion contract; fixVersion classification Keep/Revert/EDROP/NoFixVersion | chain.ts:12-77, 559-601; gitRelease.ts:43-93 | A | grep jira/fixVersion/EDROP/INTEL- : 0 hits; release-ui exists but standalone; roadmap "Eén scherm" = next |
| 2 | dbt branch names feature/bugfix/hotfix + INTEL regex enforced in editor, ticket in merge commit | chain.ts:28-35 | A | grep INTEL- 0; ci has no hotfix branch type |
| 3 | changelog/<DROP>.md per release, carried forward | chain.ts:562-566 | A | grep changelog 0 |
| 4 | Release cut by reverting unapproved merges newest-first, carry over old reverts oldest-first, re-entry by omission | chain.ts:559-566; gitRelease.ts:61-70 | D (manual git revert; Studio git.revert_on_release; merge-base reset+--no-ff+force push) | dbt-release-flow SKILL §1 |
| 5 | Release patch flow (re-enable reverted ticket / cherry-pick dev fix onto release) | chain.ts:565 | A | skill has no patch flow |
| 6 | Emergency EDROP lane, always kept | chain.ts:34-37 | A | |
| 7 | Tag scheme: DROP tag + semver twin, patch .N, year rollover, tags>=2000 ignored | gitRelease.ts:95-130 | PA (YYYY-Qx.N, no twin/emergency) | skill §1 |
| 8 | Dev-branch hygiene detectors: revert on dev = DevViolation; scope check newest two release branches | gitRelease.ts:37-41; chain.ts:599 | PA (protected branches only) | bootstrap_stack.py protected_branches |
| 9 | Release control plane: GitLab rights check, single job lock, 3-way conflict UI, announcement e-mail listing in/excluded tickets | gitRelease.ts:131-171 | A in stack (release-ui standalone) | skill arch §12 |
| 10 | Upstream raw-vault load by VaultSpeed FMC DAGs (in-DB procs, S2_dv_pool, Dataset outlets s2:// and vdp://, ingest-start dataset, ghost records) chaining into dbt_daily_load | chain.ts:660-720; vaultspeed.ts:250-300; runtime.ts:141-148 | D (landing assets fed by Studio feeder; dbt_dataset_dependencies) | arch §7 assets; grep S2_dv_pool 0 |
| 11 | Downstream handover: Cronacle SOAP ready signal, AI trigger DAGs per asset with HTTP POST | chain.ts:691-693; runtime.ts:76,132-139 | PA (asset events + notice mail only) | grep cronacle/ai_trigger 0 |
| 12 | Scheduled weekday promotion LDV->ABT (cron 09:00) | chain.ts:489-535; dbtCicd.ts:196 | A | grep daily-abt / cron 0; val fed by main merge |
| 13 | Promotion by digest without rebuild, version from image label, checksum compare, self-healing idempotent re-run | chain.ts:511-535; dbtCicd.ts:326-335 | D (one local image store, same tag redeployed) | skill §2; no skopeo |
| 14 | Supply chain: Cosign signing by digest, hardened docker run args, 18 OCI labels, Hadolint+Gitleaks image pipeline | dbtCicd.ts:430-466 | A (2 OCI labels only) | grep cosign/hadolint/gitleaks 0; Dockerfile:7 |
| 15 | Environment-gated models + env-aware DB maintenance at deploy (DBT_DEPLOY_TARGET override so ABT/PRD-only models are not dropped) | dbtCicd.ts:331, 533 | A for gating; PA for maintenance (run-operation inside dbt_release) | grep enabled/DEPLOY_TARGET in macros/models |
| 16 | Secret model: Vault for CI deploy, Airflow service creds, AppRole read-only for dbt, split nonprod/prod Vault, k8s secret per env | environments.ts; chain.ts:262-266 | PA (stand-in vault only for developer password) | arch §3, §10 |
| 17 | TLS to SingleStore with tier CA, outbound proxy, registry failover token | environments.ts; vaultspeed.ts:184; dbtCicd.ts:398-399 | A | no TLS in workshop |
| 18 | Dual Airflow 2.x/3.x sync with SSH reachability check, from exact git tag | runtime.ts:241-280 | PA (single Airflow per env; DAGs from image of tag) | skill §2 |
| 19 | Docs GitOps branch-per-env with downstream reset and semver pin | dbtCicd.ts:361-402; chain.ts:466-468 | D (docs generated at deploy into state volume) | skill §2 |
| 20 | Airflow REST API (JWT from vault creds) pauses daily load | runtime.ts:259-264 | D (CLI + /proc/1/environ) | arch §7 |

Present both sides: MR rehearsal in vdp_ci with state:modified/defer/empty (2 rounds), semver tag on dev, build image on tag, artifacts per version + per env (collibra target), release branch + merge-base reset + force push main, dbt_release (pause, maintenance, accumulated state merge, reload delegation, grants sync, orphan clean), dbt_reload blue/green OOM halving, dbt retry state, daily_load selector, run telemetry vdp_mgmt (run_results, orphaned, table_performance), failure e-mail, drift audit (not in client overview), personal dev prefix + guard + pre-commit sqlfmt/parse/bouncer, per-env DBs/service users/RBAC, docs per env, sibling container per task (OpenShift runner exists as code path), IKL_HASH_SALT optional secret.

##### Part B (client vaultspeed-automation-framework(-image) vs Rust deployer SKILL/README)
| # | client capability | file:line | status |
|---|---|---|---|
| B1 | Polling agent turns Studio ZIP into release/<gen> branch from clean main, deleting stale branch, log file per generation | vaultspeed_agent.bash:1-54; agent/README.md | A (Rust treats release/* as given, never written; no agent, no zips in stack) |
| B2 | GitLab CI chain: auto_merge_ldv, verify_merge_request dry-run merge, deploy on merge-message rules, cherry_pick of framework commits to env branch, deploy state committed back | .gitlab-ci.yml:19-150 | PA (Rust has branch model + hub, no CI in stack) |
| B3 | Deploy engine: PML then DDL non-RDV then RDV then ETL layer order, MIGR skipped, schema consolidation rewrite rules, preview/clean flags | python/deploy.py:30-141; config/vaultspeed.json | D (ledger, fail-fast, JUnit; standalone) |
| B4 | FMC DAG patching (email, pool, outlets, ghost records, incr schedule on ingest dataset) + SFTP to Airflow | deploy.py airflow_manager | PA (Rust has default_args, outlets; no ghost-record or schedule rewrite found; not run) |
| B5 | Per-env secrets via vault AppRoles and tier CA | deploy.py:48-95 | D (settings only; workshop has no TLS/vault for it) |
| B6 | Plan preview + warning when zip not on lower env; Jira key extracted from DDL comments | chain.ts:313 | PA (Rust only, standalone) |
Image repo: Dockerfile = python 3.13 + mysql client + cleartext plugin env; Jenkinsfile = dockerImagePipeline: plumbing only.

