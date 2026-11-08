"""dbt Daily Load DAG.

Production dataset-triggered DAG for daily incremental and table loads.
- Schedule: Driven by Airflow Dataset triggers (from Variable 'dbt_dataset_dependencies').
- Selection: Builds models and only runs 'unique_combination_of_columns' tests and tests tagged 'critical'
  (selector 'daily_load'), with --warn-error. On dbt Fusion --warn-error does not turn a warn-severity test into
  a failure (dbt Core does): the (environment-dependent) test severity decides.
- Execution: Cronacle ready signal fires BEFORE Airflow dataset outlets are emitted.
- Retry support: On task retry / clear (try_number > 1), automatically executes
  'dbt retry' referencing previous run state artifacts.
- Manual trigger protection: Warns and enforces that ad-hoc runs use 'dbt_vdp'.
- Drift audit (report only): right before dbt_build, `audit_environment_drift` lists relations changed or created
  outside dbt (a manual hotfix in production) and missing grants of the service account. The build recreates the
  relations from Git, which erases both the hotfix and the evidence - so the audit runs first and never stops the load
  (dbt_release runs the same audit as a gate that does).
- Levers between deploy windows (Airflow Variables, read when the task starts - utils/dbt_runtime_overrides.py):
    disabled_dbt_models       selectors appended to --exclude: the nodes stay in the lineage, they are not executed
    dbt_daily_load_dbtignore  paths/globs written to .dbtignore before dbt starts: the files leave the project
                              (build AND source freshness); every dependant must be listed too
    dbt_env_vars              environment variables for every dbt task (model versions, test severity switches)
"""

import json
import logging
import os
from datetime import datetime


from utils.compat import AirflowException, AirflowSkipException, DAG, Dataset, Param, ParamsDict, get_variable, task
from utils.dbt_config import (
    MOUNT_DIR,
    TARGET_DIR,
    DAILY_ARTIFACTS_DIR,
)
from utils.dbt_command_utils import build_daily_load_command
from utils.dbt_dag_utils import (
    validate_daily_run_trigger,
    build_failure_email_callback,
)
from utils.dbt_log_utils import FAILURE_EMAIL_RECIPIENTS
from utils.dbt_pod_utils import openshift_dbt_task, openshift_python_task
from utils.dbt_runtime_overrides import apply_overrides, read_dbtignore
from utils.dbt_state_utils import extract_successful_relations
from utils.dbt_trigger_utils import (
    generate_dataset_outlets_task,
    trigger_cronacle_vdpreadysignal_task,
)

logger = logging.getLogger(__name__)


def _get_dataset_dependencies() -> list[Dataset]:
    """Retrieve configured dataset dependency URIs from Airflow Variables."""
    try:
        raw = get_variable("dbt_dataset_dependencies", "[]")
        return [Dataset(uri) for uri in json.loads(raw)]
    except Exception as exc:
        logger.warning(f"Could not load dbt_dataset_dependencies variable: {exc}")
        return []


dataset_dependencies = _get_dataset_dependencies()

with DAG(
    dag_id="dbt_daily_load",
    start_date=datetime(2024, 1, 1),
    schedule=None if os.getenv("LOCAL") else (dataset_dependencies or None),
    catchup=False,
    max_active_runs=1,
    tags=["dbt", "production", "daily_load"],
    default_args={
        "email": FAILURE_EMAIL_RECIPIENTS,
        "email_on_failure": False,
        "on_failure_callback": build_failure_email_callback(),
    },
    params=ParamsDict(
        {
            "manual_run_override": Param(
                False,
                type="boolean",
                description=(
                    "WARNING: Ad-hoc/one-off runs should be triggered via DAG 'dbt_vdp'. "
                    "Check this box only if you intentionally wish to trigger the full "
                    "daily dataset pipeline manually."
                ),
            ),
        }
    ),
) as dag:

    @task(task_id="validate_trigger_source")
    def validate_trigger_source(**context):
        """Ensure manual triggers have explicitly accepted the override warning."""
        validate_daily_run_trigger(context)

    # Own target path (the build's manifest / run_results stay the build's); never fails: a finding is a warning here
    @openshift_dbt_task(
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

    @openshift_dbt_task(task_id="dbt_build", trigger_rule="all_success")
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

    @openshift_dbt_task(task_id="persist_artifacts_to_state", trigger_rule="all_done")
    def persist_artifacts_to_state():
        """Ensure artifacts in target dir are preserved in DAILY_ARTIFACTS_DIR for retries."""
        return [
            "bash",
            "-c",
            f"mkdir -p {DAILY_ARTIFACTS_DIR} && cp -rf {TARGET_DIR}/. {DAILY_ARTIFACTS_DIR}/",
        ]

    @openshift_python_task(
        task_id="extract_relations_to_xcom",
        trigger_rule="all_success",
        do_xcom_push=True,
    )
    def extract_relations_to_xcom():
        """Extract valid database relation targets from manifest and run_results."""
        manifest_path = os.path.join(TARGET_DIR, "manifest.json")
        run_results_path = os.path.join(TARGET_DIR, "run_results.json")

        try:
            relations = extract_successful_relations(manifest_path, run_results_path)
            return {"results": relations}
        except FileNotFoundError as err:
            raise AirflowSkipException(
                f"Artifact missing ({err}), skipping outlet generation."
            )
        except Exception as exc:
            raise AirflowException(f"Error extracting relations from artifacts: {exc}")

    # Own target path: freshness must not overwrite the build's manifest / run_results (retry + outlets read them).
    # LOG_RUN_RESULTS off: freshness results are not test results and do not belong in vdp_mgmt.dbt_run_results.
    @openshift_dbt_task(
        task_id="dbt_source_freshness",
        trigger_rule="all_done",
        target_path=os.path.join(MOUNT_DIR, "target_freshness"),
        extra_env_vars={"LOG_RUN_RESULTS": "FALSE"},
    )
    def dbt_source_freshness():
        """Source freshness against the simulation clock (sources_olist.yml: warn 1 day / error 3 days behind,
        macros/utils/olist_freshness.sql). Informational: a late or withheld source (e.g. reviews in PRD before
        Domain B is delivered) is a conversation with the source team, so it is reported here but never fails the load."""
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

    # Task instantiation and graph definition
    gate_task = validate_trigger_source()
    drift_task = audit_environment_drift()
    build_task = dbt_build()
    persist_task = persist_artifacts_to_state()
    extract_task = extract_relations_to_xcom()
    cronacle_task = trigger_cronacle_vdpreadysignal_task()
    outlets_task = generate_dataset_outlets_task(extract_task)
    freshness_task = dbt_source_freshness()

    # Workflow Order:
    # 1. Validate trigger source
    # 2. Drift audit (report only: before the build erases the evidence)
    # 3. Run dbt build
    # 4. Persist target artifacts to state (for retry capability on task clear)
    # 5. Extract relations to XCom
    # 6. Trigger Cronacle ready signal BEFORE generating Airflow dataset outlets
    # 7. Generate Airflow dataset outlets
    (
        gate_task
        >> drift_task
        >> build_task
        >> persist_task
        >> extract_task
        >> cronacle_task
        >> outlets_task
    )
    # 8. Report source freshness after the build (side branch: never blocks or fails the chain above)
    build_task >> freshness_task
