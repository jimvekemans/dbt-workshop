"""Airflow actions: runtime settings (Airflow Variables), running a scheduled job and retrying its failed part.

There is no Airflow API user in the stack. Commands go through `docker exec workshop-airflow-<env> airflow ...`;
run and task state is read from the metadata database `airflow_<env>` (tables `dag_run` and `task_instance`).
"""

from __future__ import annotations

import json
import time
import uuid
from datetime import datetime, timezone
from typing import Any, Literal

from pydantic import Field, field_validator

from . import clients
from .base import Cancelled, Ctx, Params, Result, StepFailed, action, cleaner

DAG_NAMES = {
    "dbt_daily_load": "the nightly load",
    "dbt_release": "the release run",
    "dbt_reload": "the table rebuild",
    "dbt_housekeeping": "the housekeeping run",
}
# `dbt_release` hands big tables to `dbt_reload`; a paused child would leave the release waiting forever.
UNPAUSE_ALSO = {"dbt_release": ("dbt_reload",)}
# Task ids of dbt-singlestore-workshop/airflow/dags/*.py in words the audience understands.
FRIENDLY_TASKS = {
    "audit_environment_drift": "Checking for hand-made changes",
    "dbt_build": "Building all the reports",
    "persist_artifacts_to_state": "Saving the run so it can be resumed",
    "dbt_source_freshness": "Checking that every data feed is up to date",
    "send_failure_report": "Sending the failure mail",
    "pause_daily_load": "Pausing the nightly load",
    "unpause_daily_load": "Resuming the nightly load",
    "run_operation__database_maintenance": "Tidying the database",
    "build__datum_and_staging": "Building the first layer of tables",
    "build__core_dimensions_and_fact": "Building the core tables",
    "reload__dummy_reload_date_window": "Rebuilding a large table without stopping readers",
    "build_remaining": "Building everything else",
    "run_operation__sync_grants": "Updating who may see what",
    "run_operation__clean_orphaned_models": "Removing tables nobody uses any more",
    "cleanup_temporary_artifacts": "Cleaning up temporary files",
    "init_green_table": "Preparing the new copy of the table",
    "test_green_table": "Checking the new copy of the table",
    "atomic_swap": "Swapping the new copy in",
    "cleanup_backup_table": "Removing the old copy",
    "archive_reload_artifacts": "Saving the rebuild's results",
    "cleanup_scratch_table": "Removing the unfinished copy",
    "cleanup_user_models": "Removing developers' test tables",
    "drop_old_relations": "Removing leftover tables",
}
TASK_PREFIXES = (
    ("batch_chunk_", "Loading one part of the new copy"),
    ("prep_upstream__", "Preparing the previous results"),
    ("merge_state__", "Saving the results"),
    ("reload__", "Rebuilding a large table"),
    ("build__", "Building a group of tables"),
)
# state of a task instance -> (code the UI colours, plain words)
TASK_STATES = {
    "success": ("done", "Done"),
    "failed": ("failed", "Failed"),
    "running": ("running", "Running"),
    "up_for_retry": ("running", "Trying again"),
    "restarting": ("running", "Restarting"),
    "deferred": ("running", "Running"),
    "queued": ("todo", "Waiting"),
    "scheduled": ("todo", "Waiting"),
    "upstream_failed": ("skipped", "Not run"),   # never ran because a step before it failed
    "skipped": ("skipped", "Skipped"),
}
# dbt_daily_load keeps going after a failed dbt_build (so a retry has its artifacts) and ends "success":
# the build decides.
# dbt_release ends "success" after a failed drift check (the clean-up tasks still run), so that check decides too.
KEY_TASKS = {"dbt_daily_load": ("dbt_build",), "dbt_release": ("audit_environment_drift",)}
FAILED = {"failed", "upstream_failed"}
SETTING_NAMES = {
    "dbt_env_vars": "a setting of the nightly load",
    "dbt_daily_load_dbtignore": "the list of skipped files",
    "disabled_dbt_models": "the list of switched-off tables",
    "dbt_image_tag": "the deployed version",
}


def friendly_task(task_id: str) -> str:
    """Plain words for a task id (task groups prefix it with `group.`)."""
    name = task_id.rsplit(".", 1)[-1]
    if name in FRIENDLY_TASKS:
        return FRIENDLY_TASKS[name]
    return next((words for prefix, words in TASK_PREFIXES if name.startswith(prefix)), "Working")


def dag_name(dag: str) -> str:
    return DAG_NAMES.get(dag, "the scheduled job")


def setting_name(key: str) -> str:
    return SETTING_NAMES.get(key, "a setting")


def iso(value: Any) -> str | None:
    return value.replace(tzinfo=timezone.utc).isoformat() if isinstance(value, datetime) else None


# --- reading run state from the metadata database -------------------------------------------------


def latest_run(ctx: Ctx, env: str, dag: str, run_id: str | None = None) -> dict[str, Any] | None:
    where, args = ("AND run_id = %s", (dag, run_id)) if run_id else ("", (dag,))
    rows = clients.airflow_query(
        ctx.settings,
        env,
        "SELECT run_id, state, start_date, end_date, logical_date, clear_number FROM dag_run "
        f"WHERE dag_id = %s {where} ORDER BY id DESC LIMIT 1",
        args,
    )
    return rows[0] if rows else None


def task_rows(ctx: Ctx, env: str, dag: str, run_id: str) -> list[dict[str, Any]]:
    rows = clients.airflow_query(
        ctx.settings,
        env,
        "SELECT task_id, state, start_date, end_date, try_number FROM task_instance "
        "WHERE dag_id = %s AND run_id = %s ORDER BY start_date IS NULL, start_date, id",
        (dag, run_id),
    )
    # After a clear the table keeps the earlier tries of a task too; only the newest try says how the task ended.
    newest: dict[str, int] = {}
    for row in rows:
        newest[row["task_id"]] = max(newest.get(row["task_id"], 0), row.get("try_number") or 0)
    return [row for row in rows if (row.get("try_number") or 0) == newest[row["task_id"]]]


def run_state(dag: str, run: dict[str, Any], tasks: list[dict[str, Any]]) -> str:
    """The run's state as it matters: a finished run whose key task failed counts as failed."""
    state = run["state"] or "queued"
    keys = KEY_TASKS.get(dag, ())
    if state == "success" and any(t["task_id"] in keys and t["state"] in FAILED for t in tasks):
        return "failed"
    return state


# Bookkeeping that exists once per step of a run: shown as one row with the worst state of the group and a `count`.
COLLAPSED = ("Saving the results", "Preparing the previous results")
STATE_RANK = {"failed": 4, "running": 3, "todo": 2, "skipped": 1, "done": 0}   # the worst state of a group wins


def first_attempt(run: dict[str, Any] | None, task: dict[str, Any]) -> bool:
    """Did this task end before the run was last restarted? After a retry (`clear_number` > 0) Airflow sets the run's
    start to the moment of the restart, so a finished task that ended before that keeps its state of the first
    attempt (the failure mail, for one, was sent then and is not sent again)."""
    started, ended = (run or {}).get("start_date"), task.get("end_date")
    return bool(run and run.get("clear_number") and isinstance(started, datetime) and isinstance(ended, datetime)
                and ended < started and task["state"] in ("success", "skipped"))


# Steps that only matter after something went wrong or after a change; skipped in a run where nothing failed they are
# "not needed", not a problem.
NOT_NEEDED = {"Sending the failure mail", "Cleaning up temporary files", "Removing the old copy", "Removing the unfinished copy",
              "Removing developers' test tables", "Removing leftover tables", "Removing tables nobody uses any more"}
PART_LABEL = "Loading one part of the new copy"
FINISHED_RUN = ("success", "failed")


def _part_number(task_id: str) -> int | None:
    tail = task_id.rsplit(".", 1)[-1].rsplit("_", 1)[-1]
    return int(tail) if tail.isdigit() else None


def describe_tasks(tasks: list[dict[str, Any]], run: dict[str, Any] | None = None) -> list[dict[str, Any]]:
    """The tasks in words. A step that exists many times (one per slice) and was not needed is left out; repeated
    bookkeeping tasks are one row with a `count`; after a retry a task whose state is from the first attempt says so."""
    labels = [friendly_task(t["task_id"]) for t in tasks]
    tasks = [t for t, label in zip(tasks, labels) if not (t["state"] == "skipped" and labels.count(label) > 1)]
    rows: list[dict[str, Any]] = []
    sources: list[dict[str, Any]] = []   # the task each row comes from (the first one of a collapsed group)
    nothing_failed = not any(t["state"] in FAILED for t in tasks)
    finished = bool(run) and (run.get("state") in FINISHED_RUN)
    for t in tasks:
        label = friendly_task(t["task_id"])
        state, words = TASK_STATES.get(t["state"], ("todo", "Waiting"))
        if t["state"] in (None, "scheduled", "queued", "none") and finished and not nothing_failed:
            state, words = "skipped", "Not run"   # the run is over and a step before it failed: this one never ran
        if state == "done" and first_attempt(run, t):
            words = "Done in the first attempt"
        row = {"label": label, "state": state, "state_text": words}
        if t["state"] == "skipped" and nothing_failed and label in NOT_NEEDED:
            row["not_needed"] = True
        if label in COLLAPSED:
            same = next((r for r in rows if r["label"] == label), None)
            if same is None:
                rows.append({**row, "count": 1})
                sources.append(t)
            else:
                same["count"] += 1
                if STATE_RANK.get(state, 2) > STATE_RANK.get(same["state"], 2):
                    same["state"], same["state_text"] = state, words
            continue
        rows.append(row)
        sources.append(t)
    parts = [(r, _part_number(t["task_id"])) for r, t in zip(rows, sources) if r["label"] == PART_LABEL]
    if len(parts) > 1:   # "Part 2 of 4": in the order of the numbers in the task ids, else in the order shown
        ordered = sorted(range(len(parts)), key=lambda i: (parts[i][1] is None, parts[i][1] or 0, i))
        for place, i in enumerate(ordered, 1):
            parts[i][0]["part"], parts[i][0]["parts"] = place, len(parts)
    return rows


def running_label(tasks: list[dict[str, Any]]) -> str:
    running = [t for t in tasks if t["state"] in ("running", "up_for_retry", "restarting", "deferred")]
    return friendly_task(running[-1]["task_id"]) if running else ""


def failed_label(tasks: list[dict[str, Any]]) -> str:
    failed = [t for t in tasks if t["state"] == "failed"]
    return friendly_task(failed[0]["task_id"]) if failed else ""


# --- settings (Airflow Variables) -----------------------------------------------------------------


class SetVariableParams(Params):
    key: str
    value: Any

    @field_validator("key")
    @classmethod
    def not_the_version(cls, key: str) -> str:
        if key == "dbt_image_tag":
            raise ValueError(
                "the deployed version is changed with `deploy.set_version`, which also cleans up after itself"
            )
        return key


class DeleteVariableParams(Params):
    key: str


def variable_text(value: Any) -> str:
    return json.dumps(value) if isinstance(value, (dict, list)) else str(value)


def read_variable(ctx: Ctx, env: str, key: str) -> str | None:
    proc = clients.airflow_cli(ctx, env, "variables", "get", key, check=False)
    return proc.stdout.strip() if proc.returncode == 0 else None


def merged_value(ctx: Ctx, env: str, key: str, value: Any) -> Any:
    """`dbt_env_vars` is an object of environment variables: new keys are added to what is there, never replace it."""
    if key != "dbt_env_vars" or not isinstance(value, dict):
        return value
    try:
        current = json.loads(read_variable(ctx, env, key) or "{}")
    except json.JSONDecodeError:
        current = {}
    return {**current, **value} if isinstance(current, dict) else value


@action("airflow.set_variable", SetVariableParams, lambda p: f"Changing {setting_name(p.key)}")
def set_variable(ctx: Ctx, p: SetVariableParams) -> Result:
    value = merged_value(ctx, ctx.env, p.key, p.value)
    clients.airflow_cli(ctx, ctx.env, "variables", "set", p.key, variable_text(value))
    return Result(
        f"Changed {setting_name(p.key)} in {clients.ENV_WORDS[ctx.env]}.",
        details=f"airflow variables set {p.key} {variable_text(value)}",
        touched={f"variable:{ctx.env}:{p.key}"},
    )


@action("airflow.delete_variable", DeleteVariableParams, lambda p: f"Clearing {setting_name(p.key)}")
def delete_variable(ctx: Ctx, p: DeleteVariableParams) -> Result:
    clients.airflow_cli(ctx, ctx.env, "variables", "delete", p.key, check=False)  # absent already is fine
    return Result(
        f"Cleared {setting_name(p.key)} in {clients.ENV_WORDS[ctx.env]}.",
        details=f"airflow variables delete {p.key}",
        touched={f"variable:{ctx.env}:{p.key}"},
    )


@cleaner("variable")
def clean_variable(ctx: Ctx, resource: str) -> Result:
    """Deletes the Variable. This is also right for a key added to `dbt_env_vars`: the baseline stack has none of the
    levers (`dbt_env_vars`, `dbt_daily_load_dbtignore`, `disabled_dbt_models`), so nothing that was there is lost."""
    _, env, key = resource.split(":")
    clients.airflow_cli(ctx, env, "variables", "delete", key, check=False)
    return Result(f"Cleared {setting_name(key)} in {clients.ENV_WORDS[env]}.")


# --- running a job --------------------------------------------------------------------------------


class RunDagParams(Params):
    dag: str
    conf: dict[str, Any] = Field(default_factory=dict)
    expect: Literal["success", "failed"] = "success"
    timeout: int = 900


class RetryFailedParams(Params):
    dag: str
    expect: Literal["success", "failed"] = "success"
    timeout: int = 900


def trigger(ctx: Ctx, env: str, dag: str, conf: dict[str, Any]) -> str:
    """Unpause (DAGs start paused and a run on a paused DAG just sits queued) and start a run with a unique id."""
    for name in (dag, *UNPAUSE_ALSO.get(dag, ())):
        clients.airflow_cli(ctx, env, "dags", "unpause", name, check=False)
    run_id = f"studio__{int(time.time())}_{uuid.uuid4().hex[:6]}"
    # An explicit logical date keeps `tasks clear` (retry_failed) working: runs started by hand may have none.
    logical = datetime.now(timezone.utc).isoformat()
    proc = clients.airflow_cli(
        ctx,
        env,
        "dags",
        "trigger",
        dag,
        "-r",
        run_id,
        "-c",
        json.dumps(conf),
        "-l",
        logical,
        "--no-replace-microseconds",
        check=False,
    )
    if proc.returncode != 0:
        raise StepFailed(
            f"{dag_name(dag).capitalize()} could not be started in {clients.ENV_WORDS[env]}.", proc.text[-500:]
        )
    return run_id


def wait_for_finish(
    ctx: Ctx, env: str, dag: str, run_id: str, timeout: int, after_clear: int = -1
) -> tuple[str, list[dict]]:
    """Polls until the run ends (and, after a clear, until it was really restarted); returns (state, task rows)."""
    name = dag_name(dag)

    def probe() -> tuple[str, list[dict]] | None:
        run = latest_run(ctx, env, dag, run_id)
        if run is None or (run["clear_number"] <= after_clear and run["state"] in ("success", "failed")):
            ctx.progress(f"Waiting for {name} to start")
            return None
        tasks = task_rows(ctx, env, dag, run_id)
        state = run_state(dag, run, tasks)
        if state in ("success", "failed"):
            return state, tasks
        ctx.progress(f"Working on {name}: {running_label(tasks) or 'getting started'}")
        return None

    return ctx.wait_until(probe, timeout, every=3, waiting=f"{name} did not finish")


def waited(ctx: Ctx, dag: str, run_id: str, timeout: int, after_clear: int = -1) -> tuple[str, list[dict]]:
    """`wait_for_finish`, but a timeout or a Cancel still reports what the started run changes."""
    try:
        return wait_for_finish(ctx, ctx.env, dag, run_id, timeout, after_clear)
    except (StepFailed, Cancelled) as e:
        e.touched = set(getattr(e, "touched", ())) | {f"models:{ctx.env}", "mail"}
        raise


def judge(dag: str, env: str, expect: str, state: str, tasks: list[dict], run_id: str) -> Result:
    name, where = dag_name(dag).capitalize(), clients.ENV_WORDS[env]
    stopped = failed_label(tasks)
    details = f"run {run_id}: {state}; tasks: " + ", ".join(f"{t['task_id']}={t['state']}" for t in tasks)
    touched = {f"models:{env}", "mail"}
    if state == expect == "success":
        summary = f"{name} finished without problems in {where}."
    elif state == expect == "failed":
        summary = f"{name} failed in {where}, as expected" + (f": it stopped at '{stopped}'." if stopped else ".")
    elif state == "failed":
        raise StepFailed(f"{name} failed in {where}" + (f" at '{stopped}'." if stopped else "."), details, touched)
    else:
        raise StepFailed(f"{name} was expected to fail in {where} but finished without problems.", details, touched)
    return Result(summary, details=details, touched=touched, data={"state": state})   # the run id is only in the details


@action("airflow.run_dag", RunDagParams, lambda p: f"Starting {dag_name(p.dag)}")
def run_dag(ctx: Ctx, p: RunDagParams) -> Result:
    run_id = trigger(ctx, ctx.env, p.dag, p.conf)
    state, tasks = waited(ctx, p.dag, run_id, p.timeout)
    return judge(p.dag, ctx.env, p.expect, state, tasks, run_id)


@action("airflow.retry_failed", RetryFailedParams, lambda p: f"Retrying only the failed part of {dag_name(p.dag)}")
def retry_failed(ctx: Ctx, p: RetryFailedParams) -> Result:
    run = latest_run(ctx, ctx.env, p.dag)
    if run is None or run["logical_date"] is None:
        raise StepFailed(
            f"There is no earlier run of {dag_name(p.dag)} to retry.", f"{p.dag}: no dag_run with a logical date"
        )
    date = iso(run["logical_date"])
    clients.airflow_cli(ctx, ctx.env, "tasks", "clear", p.dag, "-f", "-y", "-s", date, "-e", date)
    state, tasks = waited(ctx, p.dag, run["run_id"], p.timeout, after_clear=run["clear_number"])
    return judge(p.dag, ctx.env, p.expect, state, tasks, run["run_id"])
