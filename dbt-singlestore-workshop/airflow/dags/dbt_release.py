"""dbt Release DAG.

Release management DAG for executing predefined, multi-stage dbt deployment commands.
Features:
- Drift gate first: 'audit_environment_drift' compares the live warehouse with Git (relations changed or created
  outside dbt, missing service-account grants). A finding fails the release before the daily load is paused or a
  single model is touched: the release would otherwise overwrite a manual hotfix or die halfway on a missing grant.
- Pauses 'dbt_daily_load' before executing release operations and **fails loudly** if
  the pause cannot be acquired — prevents accidental simultaneous execution.
- Upstream model dependency expansion: the first step automatically prepends a smart
  prep task that runs 'dbt list' and builds all upstream ancestors.
- Intelligent sequential state tracking: each step's artifacts are merged into an
  accumulated state directory so later steps can use '--state' to skip already-succeeded nodes.
- Supports deferring model full-refreshes to 'dbt_reload' DAG while preserving state
  artifacts for downstream state comparisons.
- After the successful build: run-operation 'sync_grants_with_singlestore' (grants of the project's models) and
  'clean_orphaned_models' (relations of models that no longer exist in the project).
- One failure e-mail per run (task send_failure_report, utils/dbt_failure_report.py).
- Dry-run mode: preview all planned commands without executing anything or pausing the daily load.
- All artifact paths are resolved at task execution time from context['run_id'] to avoid
  Jinja template rendering issues.
"""

import logging
import os
from datetime import datetime, timedelta
from typing import Any


from airflow.providers.standard.operators.trigger_dagrun import TriggerDagRunOperator
from airflow.sdk import DAG, Param, task
from airflow.sdk.definitions.param import ParamsDict
from airflow.sdk.exceptions import AirflowException, AirflowSkipException

from utils.dbt_config import (
    MOUNT_DIR,
    RELEASE_ARTIFACTS_BASE_DIR,
    SAFE_RUN_ID_TEMPLATE,
)
from utils.dbt_command_utils import (
    build_run_operation_command,
    build_upstream_prep_command,
    format_dbt_bash_command,
)
from utils.dbt_dag_utils import REPORT_TASK_ID, failure_report_task, set_dag_paused_state
from utils.dbt_pod_utils import dbt_task, python_task, state_call
from utils.dbt_state_utils import merge_run_results_and_manifest

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Runtime path resolution
# ---------------------------------------------------------------------------


def _resolve_state_dirs(run_id: str) -> tuple[str, str]:
    """Compute run-scoped artifact directories from the live Airflow run_id.

    Returns (run_dir, accumulated_dir). Paths are resolved at task execution
    time so Jinja template strings are never relied upon inside Python pod code
    (where Airflow does not render templates).
    """
    safe_run_id = run_id.replace(":", "_").replace("+", "_")
    run_dir = os.path.join(RELEASE_ARTIFACTS_BASE_DIR, safe_run_id)
    accumulated_dir = os.path.join(run_dir, "accumulated_state")
    return run_dir, accumulated_dir


# ---------------------------------------------------------------------------
# Release step configuration
# ---------------------------------------------------------------------------

RELEASE_STEPS: list[dict[str, Any]] = [
    {
        "task_name": "build__datum_and_staging",
        "command": [
            "build",
            "-s",
            "datum",
            "stg_olist_customers",
            "stg_olist_orders",
            "--full-refresh",
        ],
        "threads": "1",
    },
    {
        "task_name": "build__core_dimensions_and_fact",
        "command": [
            "build",
            "-s",
            "dim_olist_customers",
            "fct_olist_orders",
            "--full-refresh",
        ],
        "threads": "1",
    },
    {
        "task_name": "reload__dummy_reload_date_window",
        "defer_to_reload": True,
        "model": "dummy_reload_date_window",
        "start_date": "2023-01-01",
        "end_date": "2026-01-02",
        "batch_count": 5,
        "run_tests": True,
        "cleanup_backup": True,
    },
    {
        "task_name": "build_remaining",
        "command": ["build"],
    },
]


def _build_release_doc() -> str:
    """Generate DAG-level documentation listing all release steps for the Airflow UI."""
    lines = [
        "## dbt Release Steps\n\n",
        "Gate: `dbt run-operation audit_environment_drift` (live warehouse vs. Git) runs first; a finding stops the "
        "release before the daily load is paused.\n\n",
        "Steps execute sequentially. Each step's artifacts are merged into an accumulated "
        "state directory so later steps can use `--state` to skip already-succeeded nodes.\n\n",
        "| # | Task | Command | Vars | Threads |\n",
        "|---|------|---------|------|---------|\n",
    ]
    for i, step in enumerate(RELEASE_STEPS, 1):
        cmd = " ".join(step.get("command", []))
        vars_ = f"`{step['vars']}`" if step.get("vars") else "—"
        threads = step.get("threads", "default")
        suffix = " *(→ dbt_reload)*" if step.get("defer_to_reload") else ""
        lines.append(
            f"| {i} | `{step['task_name']}`{suffix} | `dbt {cmd}` | {vars_} | `{threads}` |\n"
        )

    lines.append(
        "\n> **To change the release sequence**: edit `RELEASE_STEPS` in `dbt_release.py` and redeploy.\n\n"
        "> **Dry-run**: set `dry_run=True` at trigger time to log all planned commands "
        "without pausing the daily load or executing anything.\n"
    )
    return "".join(lines)


def expand_release_steps(steps_config: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Expand step configs into task specifications.

    Returns step metadata without embedded artifact path strings — all paths are resolved
    at task execution time via context['run_id'] to avoid Jinja rendering issues.
    For the initial step, automatically prepends a smart upstream prep entry.
    """
    expanded: list[dict[str, Any]] = []

    for step_index, step in enumerate(steps_config):
        task_name = step["task_name"]
        command = list(step.get("command", ["build"]))
        step_threads = step.get("threads")
        step_vars = step.get("vars")
        defer_to_reload = step.get("defer_to_reload", False)

        if defer_to_reload:
            expanded.append(
                {
                    "task_id": task_name,
                    "defer_to_reload": True,
                    "model": step.get("model", task_name.replace("reload__", "")),
                    "threads": step_threads,
                    "batch_count": step.get("batch_count"),
                    "start_date": step.get("start_date"),
                    "end_date": step.get("end_date"),
                    "limit": step.get("limit"),
                    "run_tests": step.get("run_tests"),
                    "cleanup_backup": step.get("cleanup_backup"),
                }
            )
            continue

        # For the initial dbt step, prepend a smart upstream prep entry
        if step_index == 0:
            expanded.append(
                {
                    "task_id": f"prep_upstream__{task_name}",
                    "is_prep": True,
                    "source_command": command,
                    "threads": step_threads,
                    "vars": step_vars,
                }
            )

        # Build command args (state paths injected at runtime — not here)
        main_command = list(command)
        if step_threads and "--threads" not in main_command:
            main_command.extend(["--threads", str(step_threads)])
        if step_vars and "--vars" not in main_command:
            main_command.extend(["--vars", str(step_vars)])

        use_state = step_index > 0
        # build_remaining skips already-succeeded nodes from accumulated state
        extra_exclude = (
            "tag:on-demand result:success"
            if task_name == "build_remaining" and use_state
            else None
        )

        expanded.append(
            {
                "task_id": task_name,
                "defer_to_reload": False,
                "is_prep": False,
                "command": main_command,
                "use_state": use_state,
                "extra_exclude": extra_exclude,
            }
        )

    return expanded


# ---------------------------------------------------------------------------
# DAG definition
# ---------------------------------------------------------------------------

with DAG(
    dag_id="dbt_release",
    start_date=datetime(2026, 1, 1),
    schedule=None,
    catchup=False,
    max_active_runs=1,
    tags=["dbt", "release", "management"],
    doc_md=_build_release_doc(),
    params=ParamsDict(
        {
            "dry_run": Param(
                False,
                type="boolean",
                description=(
                    "Preview mode: expands and logs all planned commands, resolves 'dbt list' "
                    "node targets, then stops without executing. 'dbt_daily_load' is NOT paused."
                ),
            ),
        }
    ),
) as dag:

    @dbt_task(
        task_id="audit_environment_drift",
        trigger_rule="all_success",
        target_path=os.path.join(MOUNT_DIR, "target_drift_audit"),
        extra_env_vars={"LOG_RUN_RESULTS": "FALSE"},
    )
    def audit_environment_drift_task(**context):
        """Gate: production must still match Git (macros/ops/audit_environment_drift.sql). Fails on a relation changed
        or created outside dbt, or a grant the service account lacks. Dry-run: report only."""
        dry_run = bool(context.get("params", {}).get("dry_run"))
        args = ["dbt", "run-operation", "audit_environment_drift", "--target", "run"]
        if dry_run:
            args += ["--args", "{fail_on_drift: false}"]
        return format_dbt_bash_command(args)

    @task(task_id="pause_daily_load", trigger_rule="all_success")
    def pause_daily_load_task(**context):
        """Pause dbt_daily_load before release to prevent data race conditions.

        Raises AirflowException if the pause cannot be acquired — the release
        will not proceed with an unlocked daily load pipeline.
        """
        if context.get("params", {}).get("dry_run"):
            logger.info("[DRY RUN] Skipping pause of dbt_daily_load.")
            raise AirflowSkipException(
                "Dry-run mode: dbt_daily_load will not be paused."
            )

        logger.info("Pausing dbt_daily_load before release execution.")
        success = set_dag_paused_state("dbt_daily_load", is_paused=True)
        if not success:
            raise AirflowException(
                "Failed to pause 'dbt_daily_load'. Cannot proceed with release — "
                "running both DAGs simultaneously risks data race conditions. "
                "Check Airflow metadata DB connectivity and retry."
            )

    @dbt_task(
        task_id="run_operation__database_maintenance", trigger_rule="all_success"
    )
    def run_operation__database_maintenance(**context):
        """Perform database maintenance routines prior to model compilation."""
        if context.get("params", {}).get("dry_run"):
            logger.info(
                "[DRY RUN] Would execute: dbt run-operation perform_database_maintenance --target run"
            )
            raise AirflowSkipException("Dry-run mode.")
        return format_dbt_bash_command(
            ["dbt", "run-operation", "perform_database_maintenance", "--target", "run"]
        )

    def _create_prep_task(
        task_id: str,
        source_command: list[str],
        threads: str | None,
        cli_vars: str | None,
    ):
        """Create a smart upstream prep task that resolves node targets at runtime via dbt list."""

        @dbt_task(task_id=task_id, trigger_rule="all_success")
        def _exec(**context):
            run_dir, _ = _resolve_state_dirs(context["run_id"])
            task_target_dir = os.path.join(run_dir, task_id)
            params = context.get("params", {})

            prep_cmd = build_upstream_prep_command(
                command=source_command,
                threads=threads,
                cli_vars=cli_vars,
                target_path=task_target_dir,
            )
            if params.get("dry_run"):
                logger.info(
                    f"[DRY RUN] Prep task '{task_id}' would execute:\n{prep_cmd}"
                )
                raise AirflowSkipException("Dry-run mode.")
            return prep_cmd

        return _exec()

    def _create_build_task(
        task_id: str,
        command_args: list[str],
        use_state: bool = False,
        extra_exclude: str | None = None,
    ):
        """Create a dbt build task with artifact paths resolved at execution time."""

        @dbt_task(task_id=task_id, trigger_rule="all_success")
        def _exec(**context):
            run_dir, accumulated_dir = _resolve_state_dirs(context["run_id"])
            task_target_dir = os.path.join(run_dir, task_id)
            params = context.get("params", {})

            cmd = ["dbt", *command_args]
            if use_state:
                cmd.extend(["--state", accumulated_dir])
            if extra_exclude:
                cmd.extend(["--exclude", extra_exclude])
            if "--target" not in cmd:
                cmd.extend(["--target", "run"])
            cmd.extend(["--target-path", task_target_dir])

            if params.get("dry_run"):
                logger.info(
                    f"[DRY RUN] Task '{task_id}' would execute:\n{' '.join(cmd)}"
                )
                raise AirflowSkipException("Dry-run mode.")
            return format_dbt_bash_command(cmd)

        return _exec()

    def _create_merge_state_task(task_id: str):
        """Merge step artifacts into accumulated state for downstream --state selectors."""

        @python_task(
            task_id=f"merge_state__{task_id}",
            trigger_rule="all_success",
        )
        def _merge(**context):
            params = context.get("params", {})
            if params.get("dry_run"):
                logger.info(
                    f"[DRY RUN] Would merge state artifacts for task '{task_id}'."
                )
                return None
            run_dir, accumulated_dir = _resolve_state_dirs(context["run_id"])
            source_dir = os.path.join(run_dir, task_id)
            return state_call(
                merge_run_results_and_manifest,
                source_dir=source_dir,
                accumulated_state_dir=accumulated_dir,
            )

        return _merge()

    def _create_reload_trigger_task(step: dict[str, Any]):
        """Trigger dbt_reload for a full-refresh step deferred to the reload DAG.

        Uses Jinja templating in conf — TriggerDagRunOperator renders conf as a
        template field, so SAFE_RUN_ID_TEMPLATE is correctly evaluated at runtime.
        """
        task_id = step["task_id"]
        target_path = f"{RELEASE_ARTIFACTS_BASE_DIR}/{SAFE_RUN_ID_TEMPLATE}/{task_id}"
        reload_conf: dict[str, Any] = {
            "model_name": step["model"],
            "target_path": target_path,
        }
        for opt_key in (
            "batch_count",
            "start_date",
            "end_date",
            "limit",
            "run_tests",
            "cleanup_backup",
        ):
            if step.get(opt_key) is not None:
                reload_conf[opt_key] = step[opt_key]

        return TriggerDagRunOperator(
            task_id=task_id,
            trigger_dag_id="dbt_reload",
            conf=reload_conf,
            reset_dag_run=True,
            wait_for_completion=True,
            deferrable=True,
            poke_interval=5,
            execution_timeout=timedelta(hours=5),
        )

    def _create_post_build_operation(task_id: str, macro: str):
        """run-operation of a carried macro after the successful build (skipped in dry-run)."""

        @dbt_task(task_id=task_id, trigger_rule="all_success")
        def _exec(**context):
            if context.get("params", {}).get("dry_run"):
                logger.info(f"[DRY RUN] Would execute: dbt run-operation {macro} --target run")
                raise AirflowSkipException("Dry-run mode.")
            return build_run_operation_command(macro, {})

        return _exec()

    @dbt_task(
        task_id="cleanup_temporary_artifacts",
        trigger_rule="none_failed_min_one_success",
    )
    def cleanup_artifacts_task(**context):
        """Clean up run-scoped temporary step artifacts while preserving accumulated_state."""
        run_dir, _ = _resolve_state_dirs(context["run_id"])
        return [
            "bash",
            "-c",
            f"if [ -d '{run_dir}' ]; then find '{run_dir}' -mindepth 1 -maxdepth 1 ! -name accumulated_state -exec rm -rf {{}} +; fi",
        ]

    @task(task_id="unpause_daily_load", trigger_rule="all_done")
    def unpause_daily_load_task(**context):
        """Unpause dbt_daily_load upon completion or failure — always runs via all_done."""
        if context.get("params", {}).get("dry_run"):
            logger.info("[DRY RUN] Skipping unpause (daily load was not paused).")
            raise AirflowSkipException("Dry-run mode: dbt_daily_load was not paused.")
        logger.info("Unpausing dbt_daily_load following release execution.")
        set_dag_paused_state("dbt_daily_load", is_paused=False)

    # Instantiate base flow tasks
    drift_task = audit_environment_drift_task()
    pause_task = pause_daily_load_task()
    maintenance_task = run_operation__database_maintenance()
    cleanup_task = cleanup_artifacts_task()
    unpause_task = unpause_daily_load_task()

    grants_task = _create_post_build_operation("run_operation__sync_grants", "sync_grants_with_singlestore")
    orphans_task = _create_post_build_operation("run_operation__clean_orphaned_models", "clean_orphaned_models")
    report_task = failure_report_task()

    drift_task >> pause_task >> maintenance_task

    # Connect release steps sequentially, merging state between each step
    previous_task = maintenance_task
    expanded_steps = expand_release_steps(RELEASE_STEPS)

    for step in expanded_steps:
        task_id = step["task_id"]

        if step.get("defer_to_reload"):
            step_task = _create_reload_trigger_task(step)
        elif step.get("is_prep"):
            step_task = _create_prep_task(
                task_id=task_id,
                source_command=step["source_command"],
                threads=step.get("threads"),
                cli_vars=step.get("vars"),
            )
        else:
            step_task = _create_build_task(
                task_id=task_id,
                command_args=step["command"],
                use_state=step.get("use_state", False),
                extra_exclude=step.get("extra_exclude"),
            )

        merge_task = _create_merge_state_task(task_id)
        previous_task >> step_task >> merge_task
        previous_task = merge_task

    previous_task >> grants_task >> orphans_task >> [cleanup_task, unpause_task]
    # one failure report per run, whichever step failed (unpause always runs and is not a dbt step)
    [t for t in dag.task_dict.values() if t.task_id not in ("unpause_daily_load", REPORT_TASK_ID)] >> report_task
