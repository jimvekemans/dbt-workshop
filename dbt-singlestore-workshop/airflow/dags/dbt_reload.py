import logging
import os
from datetime import datetime, timedelta


from utils.compat import AirflowException, AirflowSkipException, DAG, Param, ParamsDict, TaskGroup
from utils.dbt_config import RELOAD_ARTIFACTS_BASE_DIR
from utils.dbt_command_utils import (
    build_reload_init_command,
    build_reload_batch_command,
    build_reload_test_command,
    build_reload_swap_command,
    build_reload_cleanup_scratch_command,
    build_reload_drop_backup_command,
)
from utils.dbt_dag_utils import build_failure_email_callback
from utils.dbt_pod_utils import openshift_dbt_task, openshift_python_task
from utils.dbt_state_utils import consolidate_reload_artifacts

logger = logging.getLogger(__name__)
MAX_BATCH_SLOTS = 10


def _conf(context: dict, key: str, default=None):
    """dag_run.conf wins over DAG params."""
    dag_run = context.get("dag_run")
    conf = (dag_run.conf or {}) if dag_run else {}
    for source in (conf, context.get("params", {})):
        if source.get(key) is not None:
            return source[key]
    return default


def _target_path(context: dict) -> str:
    custom = _conf(context, "target_path")
    if custom:
        return custom
    return os.path.join(
        RELOAD_ARTIFACTS_BASE_DIR, context["run_id"].replace(":", "_").replace("+", "_")
    )


with DAG(
    dag_id="dbt_reload",
    start_date=datetime(2024, 1, 1),
    schedule=None,
    catchup=False,
    max_active_runs=1,
    tags=["dbt", "reload", "blue-green", "backfill"],
    doc_md="""
## dbt Reload: blue/green chunked backfill

A model opts in with `meta={'reload_strategy': {'type': 'date_window' | 'hkey_mod' | 'pit', 'column': '<col>'}}`.
Everything else lives in `macros/utils/reload_helpers.sql`.

1. **init_green_table**: `dbt run --full-refresh --target reload`: creates an empty `__new_<model>` and captures the compiled SQL.
2. **batch_chunk_1..N**: `reload_batch` (boundary args only). A retry doubles `parts`, splitting the batch into smaller INSERTs.
3. **test_green_table**: `dbt test --target reload` on `__new_<model>`.
4. **atomic_swap** / **cleanup_backup_table**: `reload_atomic_swap` / `reload_drop_backup`.
5. **archive_reload_artifacts**: merges init + test results for `dbt_release` state deferral.
6. **cleanup_scratch_table** (`one_failed`): `reload_cleanup_scratch`.

Boundaries: `date_window` needs `start_date` and `end_date` (exclusive); `pit` needs `limit` (rows per batch); `hkey_mod` needs nothing.
""",
    default_args={
        "email_on_failure": False,
        "on_failure_callback": build_failure_email_callback(),
    },
    params=ParamsDict(
        {
            "model_name": Param("dummy_reload_hkey_mod", type="string", description="Model with a reload_strategy to reload"),
            "batch_count": Param(5, type="integer", description="Number of batches (1 to 10)"),
            "start_date": Param(None, type=["null", "string"], description="date_window: inclusive start"),
            "end_date": Param(None, type=["null", "string"], description="date_window: exclusive end"),
            "limit": Param(None, type=["null", "integer"], description="pit: rows per batch"),
            "run_tests": Param(True, type="boolean", description="Test __new_<model> before swapping"),
            "cleanup_backup": Param(True, type="boolean", description="Drop the backup table after the swap"),
            "target_path": Param(None, type=["null", "string"], description="Artifact directory (set by dbt_release)"),
        }
    ),
) as dag:

    @openshift_dbt_task(task_id="init_green_table", trigger_rule="all_success")
    def init_green_table(**context):
        batches = int(_conf(context, "batch_count"))
        if not 1 <= batches <= MAX_BATCH_SLOTS:
            raise AirflowException(f"batch_count must be 1..{MAX_BATCH_SLOTS}")
        return build_reload_init_command(
            _conf(context, "model_name"), os.path.join(_target_path(context), "init")
        )

    with TaskGroup(group_id="batched_incremental_loads"):
        batch_tasks = []

        def _make_batch(batch_num: int):
            @openshift_dbt_task(
                task_id=f"batch_chunk_{batch_num}",
                trigger_rule="none_failed_min_one_success",
                retries=1,
                retry_delay=timedelta(seconds=5),
            )
            def _batch(**context):
                batches = int(_conf(context, "batch_count"))
                if batch_num > batches:
                    raise AirflowSkipException(f"Batch {batch_num} > batch_count ({batches})")
                parts = 2 ** (context["task_instance"].try_number - 1)
                return build_reload_batch_command(
                    _conf(context, "model_name"),
                    batch_num,
                    batches,
                    parts=parts,
                    start_date=_conf(context, "start_date"),
                    end_date=_conf(context, "end_date"),
                    limit=_conf(context, "limit"),
                )

            return _batch()

        for slot in range(1, MAX_BATCH_SLOTS + 1):
            batch_tasks.append(_make_batch(slot))
        for b_curr, b_next in zip(batch_tasks[:-1], batch_tasks[1:]):
            b_curr >> b_next

    @openshift_dbt_task(task_id="test_green_table", trigger_rule="none_failed_min_one_success")
    def test_green_table(**context):
        if not _conf(context, "run_tests", True):
            raise AirflowSkipException("run_tests=False")
        return build_reload_test_command(
            _conf(context, "model_name"), os.path.join(_target_path(context), "test")
        )

    @openshift_dbt_task(task_id="atomic_swap", trigger_rule="none_failed_min_one_success")
    def atomic_swap(**context):
        return build_reload_swap_command(_conf(context, "model_name"))

    @openshift_dbt_task(task_id="cleanup_backup_table", trigger_rule="none_failed_min_one_success")
    def cleanup_backup_table(**context):
        if not _conf(context, "cleanup_backup", True):
            raise AirflowSkipException("cleanup_backup=False")
        return build_reload_drop_backup_command(_conf(context, "model_name"))

    @openshift_python_task(
        task_id="archive_reload_artifacts",
        trigger_rule="none_failed_min_one_success",
        do_xcom_push=True,
    )
    def archive_reload_artifacts(**context):
        target_path = _target_path(context)
        consolidate_reload_artifacts(target_path)
        return {"model": _conf(context, "model_name"), "path": target_path}

    @openshift_dbt_task(task_id="cleanup_scratch_table", trigger_rule="one_failed")
    def cleanup_scratch_table(**context):
        return build_reload_cleanup_scratch_command(_conf(context, "model_name"))

    init_task = init_green_table()
    test_task = test_green_table()
    swap_task = atomic_swap()
    drop_task = cleanup_backup_table()
    archive_task = archive_reload_artifacts()
    cleanup_scratch_task = cleanup_scratch_table()

    init_task >> batch_tasks[0]
    batch_tasks >> test_task
    [*batch_tasks, test_task] >> swap_task
    swap_task >> drop_task >> archive_task
    swap_task >> archive_task
    [init_task, *batch_tasks, test_task, swap_task] >> cleanup_scratch_task
