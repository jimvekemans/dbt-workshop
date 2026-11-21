---
name: dbt-reload-pipeline
description: >-
  Runbook for the dbt_reload blue/green chunked full-refresh: model `meta.reload_strategy`, the
  reload_helpers.sql macros, the empty-table + SQL-capture hook, Airflow batch/retry handling and
  dbt_release delegation. Use when working on dbt_reload.py, reload_helpers.sql,
  get_create_table_as_sql.sql, generate_alias_name.sql or the dummy_reload_* models (dbt project:
  `dbt-singlestore-workshop/` in the repository root; stack: root Makefile, environments dev/val/prd).
---

# dbt_reload: blue/green chunked reload

## Contract
- **No reload vars exist.** The only model-level switch is `meta={'reload_strategy': {'type': ..., 'column': ...}}` (Fusion rejects unknown top-level config keys, so it lives under `meta`). Types: `date_window`, `hkey_mod`, `pit`. Models carry no batch filters.
- Everything else is macros (`macros/utils/reload_helpers.sql`) plus two project overrides in `macros/core/`:
  - `get_create_table_as_sql`: on `--target reload`, for models with a strategy, captures the compiled SQL into `<env>_vdp_mgmt.reload_state` and creates the table EMPTY (`select * from (<sql>) where 1 = 0`).
  - `generate_alias_name`: on `--target reload`, nodes with a strategy get the `__new_` alias prefix (reload ONE model at a time; other strategy models referenced by it would be prefixed too). On target `collibra` (catalogue artifacts) it returns the plain model name, so the reload prefix never appears there.

## Flow (Airflow `dbt_reload`; each step is a built-in command or one macro)
1. `dbt run -s <m> --full-refresh --target reload`: empty `__new_<m>` + captured SQL.
2. `dbt run-operation reload_batch --args '{model_name, batch, batches, parts, start_date, end_date, limit}' --target reload` for batch 1..N. Args are boundaries only:
   - `date_window`: `[start_date, end_date)` split in `batches*parts` equal slices (column cast to datetime).
   - `hkey_mod`: residue `(batch-1)+j*batches` of `batches*parts`, over `md5(cast(col as char))`.
   - `pit`: `limit` rows after `max(col)` of `__new_<m>`, in `parts` pieces.
   - Retry (OOM): Airflow sets `parts = 2**(try_number-1)`, which splits the batch into smaller INSERTs.
3. `dbt test -s <m> --target reload` (tests resolve to `__new_<m>`).
4. `reload_atomic_swap` (sequential `ALTER TABLE .. RENAME TO`, SingleStore has no multi-rename): orig -> `<m>__backup_<ts>`, `__new_<m>` -> orig, deletes the state row. Then `reload_drop_backup`.
5. `archive_reload_artifacts` merges `init/` + `test/` run_results/manifest into `target_path` so `dbt_release` (`--state accumulated_state --exclude result:success`) skips the model.
6. `cleanup_scratch_table` (`one_failed`) runs `reload_cleanup_scratch`: drops `__new_<m>` and the state row. Production is never touched before the swap.

## Params (dag_run.conf wins)
`model_name`, `batch_count` (1..10), `start_date`/`end_date` (date_window), `limit` (pit), `run_tests`, `cleanup_backup`, `target_path`. `dbt_release` steps with `defer_to_reload: True` pass these through (`model`, `batch_count`, `start_date`, `end_date`, `limit`, ...).

## Dummy models (`models/edw_dv_bdv/`, tests in `dummy_reload.yml`)
`ref_dummy_numbers` (1..100000), `dummy_loading_history`, `dummy_load_cycles` (the loading history written by the hooks `open_load_cycle` / `close_load_cycle` of `dummy_reload_pit_with_deps`; skipped on target `reload`), `dummy_reload_date_window` (1096 rows, `event_date`), `dummy_reload_hkey_mod` (100000 rows, `hkey_record`), `dummy_reload_pit_with_deps` (PIT on `datum`, `pit_key`).

## Gotchas
- In `--target reload` the graph alias already has `__new_`; `get_reload_relation_names` strips it.
- The project overrides dbt's `should_full_refresh()` (`macros/core/should_full_refresh.sql`): a model with `meta: {full_refresh_only_when_listed: true}` (today only `dummy_load_cycles`) ignores a bare `--full-refresh` and is rebuilt only when named in `--vars '{full_refresh_models: [...]}'`. Never put that meta key on a model with a `reload_strategy`: step 1 relies on `--full-refresh` to create the empty `__new_<m>`.
- Manual CLI check against the running stack: run steps 1-4 by hand from the deployed project image on `workshop_net`
  (`<tag>` = the Airflow Variable `dbt_image_tag` of that environment; `run-operation` needs `--target reload` for
  batches and `--target run` for swap/cleanup):
  ```bash
  docker run --rm --user root --network workshop_net \
    -e SINGLESTORE_HOST=singlestore -e SINGLESTORE_USER=svc_dev_dbt -e SINGLESTORE_PASS=<see init_env.sql.tmpl> \
    -e DBT_DEPLOY_TARGET=dev dbt-vdp:<tag> \
    bash -c "dbt run -s dummy_reload_date_window --full-refresh --target reload --target-path /tmp/t"
  ```
  This writes into the `dev_*` databases. In Airflow, with `DBT_RUNTIME=docker` (the workshop stack), every dbt task runs
  as a sibling container started by `utils/dbt_pod_utils.py` (image `dbt-vdp:<Variable dbt_image_tag>`, only `mnt_dbt` of
  `WORKSHOP_STATE_VOLUME` mounted); `archive_reload_artifacts` is a `python_task` that returns a `state_call` and runs
  in the Airflow container itself (OpenShift runs it in a pod).
- The swap is two sequential renames: for a fraction of a second (about 0.1 s measured; SingleStore rejected a single-statement swap) `<m>` does not exist, so the swap is not zero-downtime; nothing in the workshop queries during the swap.
- Triggering by hand: `docker exec workshop-airflow-<dev|val|prd> airflow dags trigger dbt_reload --conf '{"model_name": "dummy_reload_hkey_mod", "batch_count": 4}'`
  (UIs: dev http://localhost:8088, val 8089, prd 8090).
- There are no OOM simulation vars; to test halving, force a failing first attempt in Airflow. A reload step in
  `RELEASE_STEPS` (`dbt_release.py`) takes only `task_name`, `defer_to_reload`, `model`, `threads` and what `reload_conf` forwards
  (`batch_count`, `start_date`, `end_date`, `limit`, `run_tests`, `cleanup_backup`); any other key, `vars` included, is dropped
  (`airflow/tests/test_release_steps.py` checks it). The step used to carry `"vars": {"simulate_oom": False}`; removing it reaches an
  environment only with the next deployed tag (a deploy job copies `airflow/dags` out of the tag's image into the environment).
