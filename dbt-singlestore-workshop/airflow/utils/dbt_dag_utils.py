"""Airflow DAG coordination utilities: pausing/unpausing DAGs and the one-per-run failure report.

The mail itself (parsing, rendering, SMTP) lives in utils/dbt_failure_report.py and has no Airflow dependency; this
module only collects what the report needs from the Airflow side.
"""

import glob
import logging
import os
from datetime import datetime
from typing import Any

from utils.dbt_config import ENV, RELEASE_ARTIFACTS_BASE_DIR, TARGET_DIR
from utils.dbt_failure_report import build_and_send

logger = logging.getLogger(__name__)

REPORT_TASK_ID = "send_failure_report"
MAX_LOG_CHARS = 400_000


def set_dag_paused_state(dag_id: str, is_paused: bool) -> bool:
    """Pause or unpause a specified DAG programmatically using Airflow's DagModel or CLI fallback.

    Returns True if state was successfully modified, False otherwise.
    """
    try:
        from airflow.models.dag import DagModel
        from airflow.utils.session import provide_session

        @provide_session
        def _set_paused(session=None):
            dag_model = (
                session.query(DagModel).filter(DagModel.dag_id == dag_id).first()
            )
            if dag_model:
                dag_model.set_is_paused(is_paused=is_paused, session=session)
                logger.info(
                    f"Successfully set DAG '{dag_id}' paused state to {is_paused}"
                )
                return True
            logger.warning(
                f"DAG '{dag_id}' not found in metadata database; could not update pause state."
            )
            return False

        return _set_paused()
    except Exception as exc:
        try:
            import os
            import subprocess

            sub_env = os.environ.copy()
            if sub_env.get("AIRFLOW__DATABASE__SQL_ALCHEMY_CONN", "").startswith(
                "airflow-db-not-allowed"
            ):
                try:
                    with open("/proc/1/environ", "rb") as f:
                        for item in f.read().split(b"\0"):
                            if item.startswith(b"AIRFLOW__DATABASE__SQL_ALCHEMY_CONN="):
                                sub_env["AIRFLOW__DATABASE__SQL_ALCHEMY_CONN"] = (
                                    item.split(b"=", 1)[1].decode("utf-8", errors="ignore")
                                )
                                break
                except Exception:
                    pass

            action = "pause" if is_paused else "unpause"
            res = subprocess.run(
                ["airflow", "dags", action, dag_id],
                capture_output=True,
                text=True,
                env=sub_env,
                check=False,
            )
            if res.returncode == 0:
                logger.info(
                    f"Successfully set DAG '{dag_id}' paused state to {is_paused} via CLI fallback."
                )
                return True
            logger.warning(
                f"CLI fallback failed for DAG '{dag_id}' ({action}): {res.stderr}"
            )
        except Exception as cli_exc:
            logger.warning(f"CLI fallback error for DAG '{dag_id}': {cli_exc}")
        logger.warning(
            f"Failed to set pause state for DAG '{dag_id}' (is_paused={is_paused}): {exc}"
        )
        return False


def _failed_task_ids(context: dict[str, Any]) -> list[str]:
    """Task ids of this run that ended 'failed' (not upstream_failed), asked from the Airflow API."""
    ti = context["ti"]
    states = ti.get_task_states(dag_id=ti.dag_id, run_ids=[ti.run_id]).get(ti.run_id, {})
    return [t for t, state in states.items() if str(state) == "failed" and t != REPORT_TASK_ID]


def read_task_log_text(dag_id: str, run_id: str, task_id: str) -> str:
    """The newest attempt's log of a task, read from the local log folder (LocalExecutor: same container)."""
    from airflow.configuration import conf

    base = conf.get("logging", "base_log_folder")
    pattern = os.path.join(glob.escape(base), f"dag_id={glob.escape(dag_id)}", f"run_id={glob.escape(run_id)}",
                           f"task_id={glob.escape(task_id)}", "attempt=*.log")
    files = sorted(glob.glob(pattern), key=os.path.getmtime)
    if not files:
        return ""
    with open(files[-1], encoding="utf-8", errors="replace") as handle:
        return handle.read()[-MAX_LOG_CHARS:]


def _run_url(dag_id: str, run_id: str) -> str:
    try:
        from airflow.configuration import conf

        base = conf.get("api", "base_url", fallback="").rstrip("/")
        return f"{base}/dags/{dag_id}/runs/{run_id}" if base else ""
    except Exception:
        return ""


def send_failure_report_for_run(context: dict[str, Any]) -> bool:
    """Collect the logs of the failed task(s) of this run plus run_results.json and send ONE mail. Never raises."""
    try:
        from utils.dbt_pod_utils import get_variable

        ti, dag_run = context["ti"], context.get("dag_run")
        failed = _failed_task_ids(context) or []
        logs = [f"===== task {t} =====\n{read_task_log_text(ti.dag_id, ti.run_id, t)}" for t in failed]
        started = getattr(dag_run, "start_date", None)
        not_before = started.timestamp() - 5 if isinstance(started, datetime) else None
        safe_run_id = ti.run_id.replace(":", "_").replace("+", "_")
        # release steps write to their own target dir, the daily load and the reload tasks to the shared one
        candidates = [os.path.join(RELEASE_ARTIFACTS_BASE_DIR, safe_run_id, t, "run_results.json") for t in failed]
        candidates.append(os.path.join(TARGET_DIR, "run_results.json"))
        run_results_path = next((c for c in candidates if os.path.isfile(c)), candidates[-1])
        ctx = {
            "environment": ENV,
            "dag_id": ti.dag_id,
            "run_id": ti.run_id,
            "image_tag": get_variable("dbt_image_tag", ""),
            "run_url": _run_url(ti.dag_id, ti.run_id),
        }
        return build_and_send("\n".join(logs), ctx, run_results_path=run_results_path, not_before=not_before)
    except Exception as exc:
        logger.error(f"Failure report could not be prepared: {exc}")
        return False


def failure_report_task():
    """The final `send_failure_report` task (trigger_rule one_failed). Call it inside a `with DAG(...)` block and set
    the dbt tasks as its upstream: it runs once per DAG run in which any of them failed."""
    from airflow.sdk import task

    @task(task_id=REPORT_TASK_ID, trigger_rule="one_failed")
    def send_failure_report(**context):
        """One mail: dbt findings of the failed task(s), run_results.json, the last log lines."""
        send_failure_report_for_run(context)

    return send_failure_report()
