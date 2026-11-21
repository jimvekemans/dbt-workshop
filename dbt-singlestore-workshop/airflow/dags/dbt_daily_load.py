"""dbt Daily Load DAG.

Scheduled DAG (DBT_DAILY_LOAD_SCHEDULE, default @daily) for the daily incremental and table loads.
- Selection: Builds models and only runs 'unique_combination_of_columns' tests and tests tagged 'critical'
  (selector 'daily_load'), with --warn-error. On dbt Fusion --warn-error does not turn a warn-severity test into
  a failure (dbt Core does): the (environment-dependent) test severity decides.
- Retry support: On task retry / clear (try_number > 1), automatically executes
  'dbt retry' referencing previous run state artifacts.
- Failure report: one e-mail per failed run (task send_failure_report, utils/dbt_failure_report.py).
- Drift audit (report only): right before dbt_build, `audit_environment_drift` lists relations changed or created
  outside dbt (a manual hotfix in production) and missing grants of the service account. The build recreates the
  relations from Git, which erases both the hotfix and the evidence - so the audit runs first and never stops the load
  (dbt_release runs the same audit as a gate that does).
- Levers between deploy windows (Airflow Variables, read when the task starts - utils/dbt_runtime_overrides.py):
    disabled_dbt_models       selectors appended to --exclude: the nodes stay in the lineage, they are not executed
    dbt_daily_load_dbtignore  paths/globs written to .dbtignore before dbt starts: the files leave the project
                              (build AND source freshness); every dependant must be listed too
    dbt_env_vars              environment variables for every dbt task (model versions, test severity switches)
- Assets (Airflow Variables, both off by default; the DAG file reads them when it is PARSED - the dag processor
  re-parses it about every 30 s - not when a task starts; utils/dbt_asset_utils.py):
    dbt_dataset_dependencies  JSON list of asset URIs (s2://workshop-<env>/<database>/<object>). Non-empty: the DAG has
                              no clock schedule and starts when EVERY listed asset has a new event (the feeds have
                              landed). Empty/unset: DBT_DAILY_LOAD_SCHEDULE as before. Never list a table this DAG
                              itself announces (it would start itself). A run started by hand always works.
    dbt_emit_asset_events     true: after the artifacts are persisted, tasks `extract_refreshed_tables` and
                              `emit_asset_events` announce every table the build refreshed (status success in
                              run_results.json) as an asset event whose extra holds the result (row count, build time,
                              version). Without it those two tasks do not exist.
"""

import logging
import os
from datetime import datetime

from airflow.sdk import DAG, AssetAlias, task
from airflow.sdk.exceptions import AirflowException

from utils.dbt_asset_utils import (
    REFRESHED_ALIAS,
    add_row_counts,
    emit_events_enabled,
    make_asset,
    plan_events,
    read_asset_dependencies,
)
from utils.dbt_config import (
    MOUNT_DIR,
    TARGET_DIR,
    DAILY_ARTIFACTS_DIR,
)
from utils.dbt_command_utils import build_daily_load_command
from utils.dbt_dag_utils import failure_report_task
from utils.dbt_pod_utils import dbt_task, get_variable, python_task, state_call
from utils.dbt_runtime_overrides import apply_overrides, read_dbtignore
from utils.dbt_state_utils import extract_refreshed_tables

logger = logging.getLogger(__name__)

# Cron expression or Airflow preset; set DBT_DAILY_LOAD_SCHEDULE=none to run it by hand only
_schedule = os.getenv("DBT_DAILY_LOAD_SCHEDULE", "@daily").strip()


def _daily_schedule():
    """The assets of Variable dbt_dataset_dependencies (a list: ALL of them need a new event), else the clock schedule."""
    uris = read_asset_dependencies(get_variable)
    if uris:
        return [make_asset(uri) for uri in uris]
    return None if _schedule.lower() in ("", "none") else _schedule


# Read at parse time: the tasks exist only while the Variable is true
_emit_events = emit_events_enabled(get_variable)

with DAG(
    dag_id="dbt_daily_load",
    start_date=datetime(2024, 1, 1),
    schedule=_daily_schedule(),
    catchup=False,
    max_active_runs=1,
    tags=["dbt", "daily_load"],
) as dag:

    # Own target path (the build's manifest / run_results stay the build's); never fails: a finding is a warning here
    @dbt_task(
        task_id="audit_environment_drift",
        trigger_rule="all_success",
        target_path=os.path.join(MOUNT_DIR, "target_drift_audit"),
        extra_env_vars={"LOG_RUN_RESULTS": "FALSE"},
    )
    def audit_environment_drift():
        """Report-only drift audit (macros/ops/audit_environment_drift.sql): changes made in this environment outside
        Git, before dbt_build overwrites them. Look for '[changed outside dbt]' / '[missing grant]' in the log."""
        return [
            "bash",
            "-c",
            "dbt run-operation audit_environment_drift --target run --args '{fail_on_drift: false}' "
            "|| echo 'audit_environment_drift: the audit itself could not run (see above) - informational, "
            "the daily load is not stopped'",
        ]

    @dbt_task(task_id="dbt_build", trigger_rule="all_success")
    def dbt_build(**context):
        """Execute daily dbt build or dbt retry if task was cleared/retried."""
        ti = context["task_instance"]
        execution_date = context.get("execution_date") or context.get("logical_date")

        if ti.try_number > 1:
            logger.warning(
                f"RETRY MODE (attempt {ti.try_number}): This task will execute 'dbt retry' "
                f"using state artifacts from '{DAILY_ARTIFACTS_DIR}'. "
                "Ensure 'persist_artifacts_to_state' completed successfully on the previous attempt. "
                "If artifacts are missing or stale, clear 'persist_artifacts_to_state' first, then re-clear this task."
            )

        threads = get_variable("dbt_build_threads", None)
        cli_vars = get_variable("dbt_run_variables", "{}")
        disabled_models = get_variable("disabled_dbt_models", "")

        cmd = build_daily_load_command(
            try_number=ti.try_number,
            state_dir=DAILY_ARTIFACTS_DIR,
            target_path=TARGET_DIR,
            threads=threads,
            cli_vars=cli_vars,
            disabled_models=disabled_models,
            execution_date=execution_date,
            as_bash_cmd=True,
        )
        # Files left out of the project until the next deploy window (also for `dbt retry`: same project, same state)
        ignored = read_dbtignore(get_variable)
        if ignored:
            cmd = apply_overrides(cmd, dbtignore=ignored)
        logger.info(f"Executing daily dbt command (attempt {ti.try_number}): {cmd}")
        return cmd

    @dbt_task(task_id="persist_artifacts_to_state", trigger_rule="all_done")
    def persist_artifacts_to_state():
        """Ensure artifacts in target dir are preserved in DAILY_ARTIFACTS_DIR for retries."""
        return [
            "bash",
            "-c",
            f"mkdir -p {DAILY_ARTIFACTS_DIR} && cp -rf {TARGET_DIR}/. {DAILY_ARTIFACTS_DIR}/",
        ]

    # Own target path: freshness must not overwrite the build's manifest / run_results (retry reads them).
    # LOG_RUN_RESULTS off: freshness results are not test results and do not belong in vdp_mgmt.dbt_run_results.
    @dbt_task(
        task_id="dbt_source_freshness",
        trigger_rule="all_done",
        target_path=os.path.join(MOUNT_DIR, "target_freshness"),
        extra_env_vars={"LOG_RUN_RESULTS": "FALSE"},
    )
    def dbt_source_freshness():
        """Source freshness (thresholds in the sources_*.yml files). Informational: a late source is a conversation
        with the source team, so it is reported here but never fails the load."""
        # Stale sources: informational (exit 0). Anything that kept freshness from running at all (profile rejected,
        # connection refused, ...) still fails this task, so a broken check is not mistaken for "sources are late".
        # dbt exits 1 for both, so the difference is whether the run reached its freshness summary.
        cmd = [
            "bash",
            "-c",
            "out=$(dbt source freshness --target run 2>&1); rc=$?; printf '%s\\n' \"$out\"; "
            "if [ $rc -ne 0 ] && printf '%s' \"$out\" | grep -q \"Finished 'freshness'\"; then "
            "echo 'dbt_source_freshness: one or more sources are stale or missing (see above) - informational, "
            "the daily load is not stopped'; exit 0; fi; exit $rc",
        ]
        # Same project as the build: an ignored feed has no source definition, so no freshness check either
        # (`disabled_dbt_models` does not reach this task - excluded nodes stay in the project).
        ignored = read_dbtignore(get_variable)
        return apply_overrides(cmd, dbtignore=ignored) if ignored else cmd

    # Asset events (Variable dbt_emit_asset_events): the tasks only exist while it is true.
    # No return annotation on the callables: Airflow would read `-> dict` as "one XCom per key".
    if _emit_events:

        @python_task(
            task_id="extract_refreshed_tables",
            trigger_rule="all_done",
            do_xcom_push=True,
            skip_if_missing=True,
        )
        def extract_refreshed_tables_task(**context):
            """The tables this run's build refreshed (models with status success in run_results.json, minus models
            that failed their own tests) -> XCom. Skipped when run_results.json is missing or older than this run (the
            drift audit or the build never ran): nothing was built, nothing is announced."""
            started = getattr(context.get("dag_run"), "start_date", None)
            return state_call(
                extract_refreshed_tables,
                run_results_path=os.path.join(TARGET_DIR, "run_results.json"),
                manifest_path=os.path.join(TARGET_DIR, "manifest.json"),
                not_before=started.timestamp() - 5 if isinstance(started, datetime) else None,
            )

        @task(task_id="emit_asset_events", trigger_rule="all_success", outlets=[AssetAlias(REFRESHED_ALIAS)])
        def emit_asset_events(refreshed, outlet_events=None, **context):
            """One asset event per refreshed table, s2://workshop-<env>/<database>/<table>, `extra` = the result
            (status, rows counted in the table, build time, version). dbt reports no row count, so it is counted here."""
            plan = plan_events(add_row_counts(refreshed), image_tag=str(get_variable("dbt_image_tag", "") or ""))
            if not plan:
                logger.info("No table was refreshed in this run: no asset events.")
                return
            if outlet_events is None:
                raise AirflowException("The task context has no outlet_events: asset events cannot be emitted.")
            for uri, extra in plan:
                logger.info(f"Asset event for {uri}: {extra}")
                outlet_events[AssetAlias(REFRESHED_ALIAS)].add(make_asset(uri), extra=extra)

    # Task instantiation and graph definition
    drift_task = audit_environment_drift()
    build_task = dbt_build()
    persist_task = persist_artifacts_to_state()
    freshness_task = dbt_source_freshness()
    event_tasks = []
    if _emit_events:
        extract_task = extract_refreshed_tables_task()
        event_tasks = [extract_task, emit_asset_events(extract_task)]

    # Workflow Order:
    # 1. Drift audit (report only: before the build erases the evidence)
    # 2. Run dbt build
    # 3. Persist target artifacts to state (for retry capability on task clear)
    drift_task >> build_task >> persist_task
    # 4. Report source freshness after the build (side branch: never blocks or fails the chain above)
    build_task >> freshness_task
    # 5. Announce the refreshed tables (only with dbt_emit_asset_events): after the artifacts are persisted
    if event_tasks:
        persist_task >> event_tasks[0]
    # 6. One failure report per run, whichever task failed
    [drift_task, build_task, freshness_task, *event_tasks] >> failure_report_task()
