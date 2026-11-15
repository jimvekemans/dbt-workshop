"""dbt Housekeeping DAG (weekly, dev only).

Cleans up what developers leave behind in the dev databases: their per-user model relations (cleanup_user_models) and
relations listed in the Airflow Variable `dbt_housekeeping_relations_to_drop` (JSON list of `database.schema.table`,
default empty -> nothing dropped) via drop_old_relations. Outside dev the DAG is not created.
"""

import logging
import os
from datetime import datetime

from airflow.sdk import DAG

from utils.dbt_command_utils import build_run_operation_command
from utils.dbt_config import ENV
from utils.dbt_pod_utils import dbt_task, get_variable
from utils.dbt_runtime_overrides import parse_list

logger = logging.getLogger(__name__)

if ENV == "dev":
    with DAG(
        dag_id="dbt_housekeeping",
        start_date=datetime(2026, 1, 1),
        schedule=os.getenv("DBT_HOUSEKEEPING_SCHEDULE", "@weekly"),
        catchup=False,
        max_active_runs=1,
        tags=["dbt", "housekeeping"],
    ) as dag:

        @dbt_task(task_id="cleanup_user_models")
        def cleanup_user_models():
            """Drop the developers' model relations older than a week (macros/utils/cleanup_user_models.sql)."""
            args = {"all_pu_users": True, "keep_younger_than_days": 7}
            return build_run_operation_command("cleanup_user_models", args)

        @dbt_task(task_id="drop_old_relations", trigger_rule="all_done")
        def drop_old_relations():
            """Drop the relations named in the Variable (macros/utils/drop_old_relations.sql)."""
            relations = parse_list(get_variable("dbt_housekeeping_relations_to_drop", ""))
            if not relations:
                return ["bash", "-c", "echo 'dbt_housekeeping_relations_to_drop is empty: nothing to drop'"]
            logger.info(f"Dropping {relations}")
            return build_run_operation_command("drop_old_relations", {"relations_to_drop": relations})

        cleanup_user_models() >> drop_old_relations()
