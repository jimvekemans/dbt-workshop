"""Airflow DAG coordination, run governance, and callback utilities.

Adheres to Single Responsibility Principle: manages cross-DAG pausing/unpausing,
manual trigger validation gates, and dynamic failure notification callbacks.
"""

import logging
from typing import Any, Callable

from utils.compat import AirflowException
from utils.dbt_log_utils import (
    FAILURE_EMAIL_RECIPIENTS,
    FAILURE_EMAIL_SUBJECT,
    FAILURE_EMAIL_CONTENT,
    extract_dbt_error_lines,
    extract_dbt_warning_details,
    format_error_section,
    read_task_log_text,
)
from utils.dbt_config import ENV_UPPER

logger = logging.getLogger(__name__)


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


def validate_daily_run_trigger(context: dict[str, Any]) -> None:
    """Ensure manual execution of daily load has explicit override confirmation.

    Raises AirflowException if manually triggered without the acknowledgment flag.
    """
    dag_run = context.get("dag_run")
    if not dag_run:
        return

    # If run is scheduled or dataset triggered, no check needed
    if getattr(dag_run, "run_type", "") in ("scheduled", "dataset_triggered"):
        return

    params = context.get("params", {})
    override_accepted = params.get("manual_run_override", False)

    if not override_accepted:
        raise AirflowException(
            "MANUAL TRIGGER BLOCKED: The 'dbt_daily_load' DAG is reserved for dataset-triggered "
            "scheduled production runs. For ad-hoc or manual dbt runs, please use DAG 'dbt_vdp'. "
            "To force a manual run of 'dbt_daily_load', enable the 'manual_run_override' parameter."
        )

    logger.warning(
        "MANUAL OVERRIDE DETECTED: 'dbt_daily_load' was manually triggered with manual_run_override=True."
    )


def build_failure_email_callback(
    default_recipients: list[str] | None = None,
) -> Callable[[dict[str, Any]], None]:
    """Factory creating an on_failure_callback that respects DAG/task params

    for recipient overrides and toggle flags.
    """

    def _callback(context: dict[str, Any]) -> None:
        params = context.get("params", {})
        # Check if email is explicitly disabled in parameters
        send_email_param = params.get("send_email_on_failure", True)
        if not send_email_param:
            logger.info("send_email_on_failure is False; skipping failure email.")
            return

        # Determine recipients: param override -> default_recipients -> global default
        recipient_param = params.get("failure_email_recipients")
        if recipient_param:
            if isinstance(recipient_param, str):
                recipients = [
                    r.strip() for r in recipient_param.split(",") if r.strip()
                ]
            else:
                recipients = list(recipient_param)
        elif default_recipients:
            recipients = default_recipients
        else:
            recipients = FAILURE_EMAIL_RECIPIENTS

        ti = context.get("task_instance")
        dag = context.get("dag")
        dag_id = dag.dag_id if dag else "unknown_dag"
        task_id = ti.task_id if ti else "unknown_task"
        run_id = context.get("run_id", "")
        log_url = ti.log_url if ti else ""

        error_section = ""
        if ti:
            try:
                log_text = read_task_log_text(ti)
                error_lines = extract_dbt_error_lines(log_text)
                warning_details = extract_dbt_warning_details(log_text)
                error_section = format_error_section(error_lines, warning_details)
            except Exception as err:
                logger.warning(f"Failed to parse error section for email: {err}")

        subject = FAILURE_EMAIL_SUBJECT.format(
            dag_id=dag_id, task_id=task_id, ENV=ENV_UPPER
        )
        content = (
            FAILURE_EMAIL_CONTENT.format(
                task_id=task_id,
                dag_id=dag_id,
                run_id=run_id,
                log_url=log_url,
            )
            + error_section
        )

        try:
            from airflow.utils.email import send_email

            send_email(to=recipients, subject=subject, html_content=content)
            logger.info(f"Sent failure email to {recipients} for {dag_id}.{task_id}")
        except Exception as exc:
            logger.error(f"Failed to send failure email: {exc}")

    return _callback
