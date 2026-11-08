"""Airflow actions via the docker CLI: `docker exec airflow-<env> airflow ...` (needs /var/run/docker.sock)."""
from __future__ import annotations

import json
import shlex
import time
from typing import Any

from .base import Action, ActionError, Ctx, Result, register, require

TERMINAL = {"success", "failed"}

# DAGs whose run state hides the result that matters. dbt_daily_load keeps going after a failed dbt_build
# (persist_artifacts_to_state runs with trigger_rule all_done so a retry has artifacts, and the chain after it
# succeeds), so the run ends "success" even when the build failed. For these DAGs the key task decides the outcome.
# dbt_release needs no entry: its steps chain with all_success and its cleanup ends upstream_failed, failing the run.
KEY_TASKS = {"dbt_daily_load": "dbt_build"}
FAILED_TASK_STATES = {"failed", "upstream_failed"}


def container(ctx: Ctx, env: str) -> str:
    if env not in ("dev", "prod"):
        raise ActionError(f"env must be dev|prod, got {env!r}")
    return f"{ctx.settings.airflow_container_prefix}{env}"


def _parse_json_list(text: str) -> list[dict[str, Any]] | None:
    """The airflow CLI prints log lines such as `[warning  ] ...` on stdout before the JSON: try every `[`."""
    decoder = json.JSONDecoder()
    pos = text.find("[")
    while pos != -1:
        try:
            value, _ = decoder.raw_decode(text[pos:])
            if isinstance(value, list):
                return value
        except json.JSONDecodeError:
            pass
        pos = text.find("[", pos + 1)
    return None


def _iso(value: Any) -> str | None:
    return value.isoformat() if hasattr(value, "isoformat") else (str(value) if value else None)


def _query_airflow_db(ctx: Ctx, env: str, sql: str, args: tuple) -> list[dict[str, Any]] | None:
    """Rows from the Airflow metadata DB (MySQL `airflow_<env>`), or None when it is not reachable (e.g. the studio
    runs outside compose) - the callers then fall back to the Airflow CLI."""
    import pymysql   # local import: the action registry must load without a DB driver configured
    s = ctx.settings
    if ctx.cache.get("airflow_db_down_until", 0) > time.time():
        return None
    try:
        conn = pymysql.connect(host=s.airflow_db_host, port=s.airflow_db_port, user=s.airflow_db_user,
                               password=s.airflow_db_password, database=f"{s.airflow_db_prefix}{env}",
                               connect_timeout=3, autocommit=True, cursorclass=pymysql.cursors.DictCursor)
    except pymysql.MySQLError:
        ctx.cache["airflow_db_down_until"] = time.time() + 60   # do not pay the connect timeout on every poll
        return None
    try:
        with conn.cursor() as cur:
            cur.execute(sql, args)
            return list(cur.fetchall())
    except pymysql.MySQLError:
        return None
    finally:
        conn.close()


def runs_from_db(ctx: Ctx, env: str, dag: str, limit: int = 20) -> list[dict[str, Any]] | None:
    """Newest runs first, straight from `dag_run`: one cheap query instead of a whole Airflow CLI process."""
    rows = _query_airflow_db(ctx, env, "SELECT run_id, state, start_date, end_date FROM dag_run WHERE dag_id = %s "
                                       "ORDER BY id DESC LIMIT %s", (dag, limit))
    if rows is None:
        return None
    return [{"dag_id": dag, "run_id": r["run_id"], "state": r["state"], "start_date": _iso(r["start_date"]),
             "end_date": _iso(r["end_date"])} for r in rows]


def task_state(ctx: Ctx, env: str, dag: str, run_id: str, task_id: str) -> str | None:
    """State of one task of a run: `task_instance` in the metadata DB, the CLI as fallback; None if unknown."""
    rows = _query_airflow_db(ctx, env, "SELECT state FROM task_instance WHERE dag_id = %s AND run_id = %s AND task_id = %s "
                                       "ORDER BY map_index LIMIT 1", (dag, run_id, task_id))
    if rows is not None:
        return rows[0]["state"] if rows else None
    proc = ctx.sh(["docker", "exec", container(ctx, env), "airflow", "tasks", "states-for-dag-run", dag, run_id,
                   "-o", "json"], check=False, timeout=60)
    for t in (_parse_json_list(proc.stdout) or []) if proc.returncode == 0 else []:
        if t.get("task_id") == task_id:
            return t.get("state")
    return None


def run_outcome(ctx: Ctx, env: str, dag: str, run: dict[str, Any]) -> tuple[str, str]:
    """(state, note) of a run as it matters: a finished run of a KEY_TASKS DAG whose key task failed counts as failed."""
    state = run.get("state") or "queued"
    key = KEY_TASKS.get(dag)
    if not key or state not in TERMINAL:
        return state, ""
    ts = task_state(ctx, env, dag, run.get("run_id", ""), key)
    if ts in FAILED_TASK_STATES:
        return "failed", f"{key} {ts} (the run itself ended {state})"
    return state, ""


def list_runs(ctx: Ctx, env: str, dag: str) -> list[dict[str, Any]]:
    """Runs of a DAG: the metadata DB first (newest first), the Airflow CLI as fallback."""
    runs = runs_from_db(ctx, env, dag)
    if runs is not None:
        return runs
    return list_runs_cli(ctx, env, dag)


def list_runs_cli(ctx: Ctx, env: str, dag: str) -> list[dict[str, Any]]:
    """Airflow 3 takes dag_id positionally, 2.x uses -d; try both."""
    for argv in (["dags", "list-runs", dag, "-o", "json"], ["dags", "list-runs", "-d", dag, "-o", "json"]):
        proc = ctx.sh(["docker", "exec", container(ctx, env), "airflow", *argv], check=False, timeout=60)
        if proc.returncode == 0:
            runs = _parse_json_list(proc.stdout)
            if runs is not None:
                return runs
    return []


# Airflow Variables the studio sets as operational levers (utils/dbt_runtime_overrides.py in the dbt project). None of
# them exists in a fresh stack, so deleting them is the undo, and a live Reset deletes all of them.
LEVER_VARIABLES = ("disabled_dbt_models", "dbt_daily_load_dbtignore", "dbt_env_vars")


def variable_text(value: Any) -> str:
    return json.dumps(value) if isinstance(value, (dict, list)) else str(value)


def variable_envs(env: str) -> list[str]:
    if env == "both":
        return ["dev", "prod"]
    if env not in ("dev", "prod"):
        raise ActionError(f"env must be dev|prod|both, got {env!r}")
    return [env]


def read_variables(ctx: Ctx, env: str, keys: tuple[str, ...] = LEVER_VARIABLES) -> dict[str, str] | None:
    """Current values of the lever Variables (metadata DB `variable` table); None when that is not reachable or a
    value is encrypted (fernet) - the caller then shows nothing rather than guessing."""
    marks = ", ".join(["%s"] * len(keys))
    rows = _query_airflow_db(ctx, env, f"SELECT `key`, val, is_encrypted FROM variable WHERE `key` IN ({marks})", keys)
    if rows is None or any(r.get("is_encrypted") for r in rows):
        return None
    return {r["key"]: r["val"] for r in rows}


@register
class AirflowVariable(Action):
    """Set (or `delete: true`) an Airflow Variable: `{action: airflow_variable, env: prod, key: disabled_dbt_models,
    value: ["source:raw_carrier+"]}`. Lists and objects are stored as JSON. Read by the DAGs when a task starts."""
    name = "airflow_variable"

    def render(self, p, ctx):
        require(p, "env", "key")
        if p.get("delete"):
            return [f"docker exec {container(ctx, e)} airflow variables delete {p['key']}" for e in variable_envs(p["env"])]
        require(p, "value")
        text = shlex.quote(variable_text(p["value"]))
        return [f"docker exec {container(ctx, e)} airflow variables set {p['key']} {text}" for e in variable_envs(p["env"])]

    def run(self, p, ctx):
        out = []
        for e in variable_envs(p["env"]):
            if p.get("delete"):
                # absent already is fine: deleting is how a lever is switched off
                ctx.sh(["docker", "exec", container(ctx, e), "airflow", "variables", "delete", p["key"]], check=False, timeout=60)
                out.append(f"{p['key']}@{e} deleted")
            else:
                ctx.sh(["docker", "exec", container(ctx, e), "airflow", "variables", "set", p["key"],
                        variable_text(p["value"])], timeout=60)
                out.append(f"{p['key']}@{e} = {variable_text(p['value'])}")
        return Result(output="; ".join(out))


@register
class AirflowTrigger(Action):
    name = "airflow_trigger"

    def render(self, p, ctx):
        require(p, "env", "dag")
        conf = json.dumps(p.get("conf") or {})
        return [f"docker exec {container(ctx, p['env'])} airflow dags unpause {p['dag']} && "
                f"docker exec {container(ctx, p['env'])} airflow dags trigger {p['dag']} --run-id <studio_run> --conf {shlex.quote(conf)}"]

    def run(self, p, ctx):
        env, dag = p["env"], p["dag"]
        run_id = f"studio__{int(time.time())}"
        conf = json.dumps(p.get("conf") or {})
        if p.get("unpause", True):   # DAGs are created paused; a run triggered on a paused DAG just sits queued
            # dbt_release defers to dbt_reload (TriggerDagRunOperator): that child DAG must not be paused either
            for d in [dag, *(p.get("unpause_also") or [])]:
                ctx.sh(["docker", "exec", container(ctx, env), "airflow", "dags", "unpause", d], check=False)
        ctx.sh(["docker", "exec", container(ctx, env), "airflow", "dags", "trigger", dag, "--run-id", run_id, "--conf", conf])
        if not p.get("wait", True):
            return Result(output=f"{dag} triggered ({run_id})", data={"run_id": run_id})
        deadline = time.time() + int(p.get("timeout", 900))
        state, run = "queued", {}
        while time.time() < deadline:
            ctx.sleep(5)
            runs = [r for r in list_runs(ctx, env, dag) if r.get("run_id") == run_id]
            run = runs[0] if runs else {}
            state = run.get("state") or "queued"
            ctx.say(f"  {dag}: {state}")
            if state in TERMINAL:
                break
        else:
            raise ActionError(f"{dag} did not finish within {p.get('timeout', 900)}s (last state {state})")
        state, note = run_outcome(ctx, env, dag, {**run, "run_id": run_id, "state": state})
        if note:
            ctx.say(f"  {dag}: {note}")
        if state != "success" and not p.get("allow_failure"):
            raise ActionError(f"{dag} finished with state {state}" + (f": {note}" if note else ""))
        return Result(ok=state == "success", output=f"{dag}: {state}" + (f" - {note}" if note else ""),
                      data={"run_id": run_id, "state": state})
