"""dbt On-Demand DAG: build tables on request, by hand.

No schedule, created paused. Trigger it with parameters (Airflow UI form, or
`airflow dags trigger dbt_on_demand --conf '{"select": "tag:on-demand"}'`):

    select               dbt selection (default `tag:on-demand`, the models the daily load leaves out). Space = either,
                         a comma inside one word = both. Required: an empty value is refused, never "everything".
    exclude              optional dbt exclusion, same syntax
    full_refresh_models  optional list of model names, passed as `--vars '{full_refresh_models: [...]}'` (only when
                         given; a model or macro of the project decides what it does with the var)

It runs `dbt build --select ... [--exclude ...] --target run` through the same runner as the daily load. An empty
selection is refused, and so is one that matches nothing (a typo: dbt would build nothing and still exit 0), and the
failure mail says so. The Variable `dbt_env_vars` applies too (`disabled_dbt_models` and `dbt_daily_load_dbtignore`
belong to the daily load and do not). Own target folder and own state folder (utils/dbt_config.py): nothing here is read by the daily load's retry.
Retry: a task that was cleared (try_number > 1) runs `dbt retry` from the state of the previous attempt, as the daily
load does; `persist_artifacts_to_state` keeps that state. A failure sends the usual failure mail (send_failure_report).
"""

import logging
from datetime import datetime

from airflow.sdk import DAG, Param
from airflow.sdk.definitions.param import ParamsDict

from utils.dbt_command_utils import build_on_demand_command
from utils.dbt_config import ON_DEMAND_ARTIFACTS_DIR, ON_DEMAND_TARGET_DIR
from utils.dbt_dag_utils import failure_report_task
from utils.dbt_pod_utils import dbt_task

logger = logging.getLogger(__name__)

DEFAULT_SELECT = "tag:on-demand"


def _conf(context: dict, key: str, default=None):
    """dag_run.conf wins over DAG params (as in dbt_reload)."""
    dag_run = context.get("dag_run")
    conf = (getattr(dag_run, "conf", None) or {}) if dag_run else {}
    for source in (conf, context.get("params") or {}):
        if source.get(key) is not None:
            return source[key]
    return default


with DAG(
    dag_id="dbt_on_demand",
    start_date=datetime(2024, 1, 1),
    schedule=None,
    catchup=False,
    max_active_runs=1,
    is_paused_upon_creation=True,
    tags=["dbt", "on_demand"],
    params=ParamsDict(
        {
            "select": Param(DEFAULT_SELECT, type="string", description="dbt selection, e.g. tag:on-demand"),
            "exclude": Param(None, type=["null", "string"], description="Optional dbt exclusion"),
            "full_refresh_models": Param(
                None,
                type=["null", "array"],
                items={"type": "string"},
                description="Optional model names passed as --vars '{full_refresh_models: [...]}'",
            ),
        }
    ),
) as dag:

    @dbt_task(task_id="dbt_build", trigger_rule="all_success", target_path=ON_DEMAND_TARGET_DIR)
    def dbt_build(**context):
        """dbt build of the selection, or dbt retry when this task was cleared / retried."""
        ti = context["task_instance"]
        try:
            cmd = build_on_demand_command(
                select=_conf(context, "select", DEFAULT_SELECT),
                exclude=_conf(context, "exclude"),
                full_refresh_models=_conf(context, "full_refresh_models"),
                try_number=ti.try_number,
            )
        except ValueError as exc:
            # The failure mail shows the last log lines: put the reason there, as a line of its own
            logger.error(f"Nothing was built: the request is refused. {exc}")
            raise
        if ti.try_number > 1:
            logger.warning(
                f"RETRY MODE (attempt {ti.try_number}): 'dbt retry' from the state in '{ON_DEMAND_ARTIFACTS_DIR}'. "
                "If persist_artifacts_to_state did not finish in the previous attempt, clear it first."
            )
        logger.info(f"Executing on-demand dbt command (attempt {ti.try_number}): {cmd}")
        return cmd

    @dbt_task(task_id="persist_artifacts_to_state", trigger_rule="all_done", target_path=ON_DEMAND_TARGET_DIR)
    def persist_artifacts_to_state():
        """Keep this run's artifacts in ON_DEMAND_ARTIFACTS_DIR for `dbt retry` (not the daily load's folder)."""
        return [
            "bash",
            "-c",
            f"mkdir -p {ON_DEMAND_ARTIFACTS_DIR} && cp -rf {ON_DEMAND_TARGET_DIR}/. {ON_DEMAND_ARTIFACTS_DIR}/",
        ]

    build_task = dbt_build()
    persist_task = persist_artifacts_to_state()

    build_task >> persist_task
    # One failure mail per run; it reads run_results.json from this DAG's own target folder
    build_task >> failure_report_task(target_dir=ON_DEMAND_TARGET_DIR)
