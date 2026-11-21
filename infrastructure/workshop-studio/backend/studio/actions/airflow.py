"""Airflow actions: runtime settings (Airflow Variables), running a scheduled job and retrying its failed part, and the
scheduler's own switches: pausing a job, waiting until a setting that is read when a job is parsed has taken effect,
letting a feed report in (an asset event) and waiting for a run that started by itself.

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

from .. import names
from . import clients
from .base import Cancelled, Ctx, Params, Result, StepFailed, action, cleaner

# Every plain name carries its article and is a "de" word: sentences can say "van {name}" and start with "{Name}".
DAG_NAMES = {
    "dbt_daily_load": "de nachtelijke load",
    "dbt_release": "de release run",
    "dbt_reload": "de herbouw van de tabellen",
    "dbt_housekeeping": "de opkuisrun",
    "dbt_on_demand": "de build op aanvraag",
    "report_refreshed_notice": "de verwittiging over ververste tabellen",
}
# Created paused by design (the notice must be switched on, the on-demand build waits for a request): a demo that
# starts them leaves them paused again (cleaner `dag`).
PAUSED_AT_REST = ("dbt_on_demand", "report_refreshed_notice")
# `dbt_release` hands big tables to `dbt_reload`; a paused child would leave the release waiting forever.
UNPAUSE_ALSO = {"dbt_release": ("dbt_reload",)}
# Task ids of dbt-singlestore-workshop/airflow/dags/*.py in words the audience understands.
FRIENDLY_TASKS = {
    "audit_environment_drift": "Nakijken op manuele wijzigingen",
    "dbt_build": "Alle rapporten builden",
    "persist_artifacts_to_state": "De run bewaren zodat hij hervat kan worden",
    "dbt_source_freshness": "Nakijken of elke feed recente data heeft",
    "send_failure_report": "De foutmail versturen",
    "pause_daily_load": "De nachtelijke load pauzeren",
    "unpause_daily_load": "De nachtelijke load hervatten",
    "run_operation__database_maintenance": "De database opkuisen",
    "build__datum_and_staging": "De eerste laag tabellen builden",
    "build__core_dimensions_and_fact": "De kerntabellen builden",
    "reload__dummy_reload_date_window": "Een grote tabel herbouwen zonder de lezers te storen",
    "build_remaining": "Al de rest builden",
    "run_operation__sync_grants": "Bijwerken wie wat mag zien",
    "run_operation__clean_orphaned_models": "Tabellen verwijderen die niemand nog gebruikt",
    "cleanup_temporary_artifacts": "Tijdelijke bestanden opkuisen",
    "init_green_table": "De nieuwe kopie van de tabel klaarzetten",
    "test_green_table": "De nieuwe kopie van de tabel nakijken",
    "atomic_swap": "De nieuwe kopie in de plaats zetten",
    "cleanup_backup_table": "De oude kopie verwijderen",
    "archive_reload_artifacts": "De resultaten van de herbouw bewaren",
    "cleanup_scratch_table": "De onafgewerkte kopie verwijderen",
    "cleanup_user_models": "De testtabellen van de developers verwijderen",
    "drop_old_relations": "Restanten verwijderen",
    "extract_refreshed_tables": "Opsommen welke tabellen ververst zijn",
    "emit_asset_events": "De ververste tabellen melden",
    "send_refreshed_notice": "De verwittiging versturen",
}
# Where the same task id means something else: the on-demand build builds what was asked for, not everything.
DAG_TASKS = {"dbt_on_demand": {"dbt_build": "De gevraagde tabellen builden"}}
TASK_PREFIXES = (
    ("batch_chunk_", "Eén deel van de nieuwe kopie laden"),
    ("prep_upstream__", "De vorige resultaten klaarzetten"),
    ("merge_state__", "De resultaten bewaren"),
    ("reload__", "Een grote tabel herbouwen"),
    ("build__", "Een groep tabellen builden"),
)
# state of a task instance -> (code the UI colours, plain words)
TASK_STATES = {
    "success": ("done", "Klaar"),
    "failed": ("failed", "Mislukt"),
    "running": ("running", "Bezig"),
    "up_for_retry": ("running", "Wacht op retry"),
    "restarting": ("running", "Herstart"),
    "deferred": ("running", "Bezig"),
    "queued": ("todo", "Wacht"),
    "scheduled": ("todo", "Wacht"),
    "upstream_failed": ("skipped", "Niet gerund"),   # never ran because a step before it failed
    "skipped": ("skipped", "Overgeslagen"),
}
# dbt_daily_load keeps going after a failed dbt_build (so a retry has its artifacts) and ends "success":
# the build decides.
# dbt_release ends "success" after a failed drift check (the clean-up tasks still run), so that check decides too.
KEY_TASKS = {"dbt_daily_load": ("dbt_build",), "dbt_release": ("audit_environment_drift",), "dbt_on_demand": ("dbt_build",)}
FAILED = {"failed", "upstream_failed"}
SETTING_NAMES = {
    "dbt_env_vars": "een instelling van de nachtelijke load",
    "dbt_daily_load_dbtignore": "de lijst met overgeslagen bestanden",
    "disabled_dbt_models": "de lijst met uitgeschakelde tabellen",
    "dbt_image_tag": "de gedeployde versie",
    "dbt_dataset_dependencies": "de lijst met feeds waarop de nachtelijke load wacht",
    "dbt_emit_asset_events": "de schakelaar die ververste tabellen meldt",
}


def friendly_task(task_id: str, dag: str | None = None) -> str:
    """Plain words for a task id (task groups prefix it with `group.`)."""
    name = task_id.rsplit(".", 1)[-1]
    if name in DAG_TASKS.get(dag or "", {}):
        return DAG_TASKS[dag][name]
    if name in FRIENDLY_TASKS:
        return FRIENDLY_TASKS[name]
    return next((words for prefix, words in TASK_PREFIXES if name.startswith(prefix)), "Bezig")


def dag_name(dag: str) -> str:
    return DAG_NAMES.get(dag, "de geplande job")


def setting_name(key: str) -> str:
    return SETTING_NAMES.get(key, "een instelling")


def upper_first(text: str) -> str:
    """The first letter in capitals, the rest as it is (`str.capitalize` would lowercase the rest)."""
    return text[:1].upper() + text[1:]


def plural(n: int, one: str, many: str) -> str:
    return f"{names.format_number(n)} {one if n == 1 else many}"


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
COLLAPSED = ("De resultaten bewaren", "De vorige resultaten klaarzetten")
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
NOT_NEEDED = {"De foutmail versturen", "Tijdelijke bestanden opkuisen", "De oude kopie verwijderen",
              "De onafgewerkte kopie verwijderen", "De testtabellen van de developers verwijderen", "Restanten verwijderen",
              "Tabellen verwijderen die niemand nog gebruikt"}
PART_LABEL = "Eén deel van de nieuwe kopie laden"
FINISHED_RUN = ("success", "failed")


def _part_number(task_id: str) -> int | None:
    tail = task_id.rsplit(".", 1)[-1].rsplit("_", 1)[-1]
    return int(tail) if tail.isdigit() else None


def describe_tasks(
    tasks: list[dict[str, Any]], run: dict[str, Any] | None = None, dag: str | None = None
) -> list[dict[str, Any]]:
    """The tasks in words. A step that exists many times (one per slice) and was not needed is left out; repeated
    bookkeeping tasks are one row with a `count`; after a retry a task whose state is from the first attempt says so."""
    labels = [friendly_task(t["task_id"], dag) for t in tasks]
    tasks = [t for t, label in zip(tasks, labels) if not (t["state"] == "skipped" and labels.count(label) > 1)]
    rows: list[dict[str, Any]] = []
    sources: list[dict[str, Any]] = []   # the task each row comes from (the first one of a collapsed group)
    nothing_failed = not any(t["state"] in FAILED for t in tasks)
    finished = bool(run) and (run.get("state") in FINISHED_RUN)
    for t in tasks:
        label = friendly_task(t["task_id"], dag)
        state, words = TASK_STATES.get(t["state"], ("todo", "Wacht"))
        if t["state"] in (None, "scheduled", "queued", "none") and finished and not nothing_failed:
            state, words = "skipped", "Niet gerund"   # the run is over and a step before it failed: this one never ran
        if state == "done" and first_attempt(run, t):
            words = "Klaar bij de eerste poging"
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


def running_label(tasks: list[dict[str, Any]], dag: str | None = None) -> str:
    running = [t for t in tasks if t["state"] in ("running", "up_for_retry", "restarting", "deferred")]
    return friendly_task(running[-1]["task_id"], dag) if running else ""


def failed_label(tasks: list[dict[str, Any]], dag: str | None = None) -> str:
    failed = [t for t in tasks if t["state"] == "failed"]
    return friendly_task(failed[0]["task_id"], dag) if failed else ""


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


@action("airflow.set_variable", SetVariableParams, lambda p: f"{upper_first(setting_name(p.key))} aanpassen")
def set_variable(ctx: Ctx, p: SetVariableParams) -> Result:
    value = merged_value(ctx, ctx.env, p.key, p.value)
    clients.airflow_cli(ctx, ctx.env, "variables", "set", p.key, variable_text(value))
    return Result(
        f"{upper_first(setting_name(p.key))} is aangepast in {clients.ENV_WORDS[ctx.env]}.",
        details=f"airflow variables set {p.key} {variable_text(value)}",
        touched={f"variable:{ctx.env}:{p.key}"},
    )


@action("airflow.delete_variable", DeleteVariableParams, lambda p: f"{upper_first(setting_name(p.key))} wissen")
def delete_variable(ctx: Ctx, p: DeleteVariableParams) -> Result:
    clients.airflow_cli(ctx, ctx.env, "variables", "delete", p.key, check=False)  # absent already is fine
    return Result(
        f"{upper_first(setting_name(p.key))} is gewist in {clients.ENV_WORDS[ctx.env]}.",
        details=f"airflow variables delete {p.key}",
        touched={f"variable:{ctx.env}:{p.key}"},
    )


@cleaner("variable")
def clean_variable(ctx: Ctx, resource: str) -> Result:
    """Deletes the Variable. This is also right for a key added to `dbt_env_vars`: the baseline stack has none of the
    levers (`dbt_env_vars`, `dbt_daily_load_dbtignore`, `disabled_dbt_models`), so nothing that was there is lost."""
    _, env, key = resource.split(":")
    if key in PARSE_TIME_SETTINGS:
        # Read when the DAG file is parsed (up to about 30 s later): put both back at once and wait for the parse, so
        # the next scenario does not meet a nightly load that still waits for feeds or announces tables.
        for setting in PARSE_TIME_SETTINGS:
            clients.airflow_cli(ctx, env, "variables", "delete", setting, check=False)
        wait_for_shape(ctx, env, NIGHTLY, schedule="clock", lacks=EMIT_TASKS, missing_ok=True)
    else:
        clients.airflow_cli(ctx, env, "variables", "delete", key, check=False)
    return Result(f"{upper_first(setting_name(key))} is gewist in {clients.ENV_WORDS[env]}.")


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
            f"{upper_first(dag_name(dag))} kon niet gestart worden in {clients.ENV_WORDS[env]}.", proc.text[-500:]
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
            ctx.progress(f"Wachten tot {name} start")
            return None
        tasks = task_rows(ctx, env, dag, run_id)
        state = run_state(dag, run, tasks)
        if state in ("success", "failed"):
            return state, tasks
        ctx.progress(f"Bezig met {name}: {running_label(tasks, dag) or 'opstarten'}")
        return None

    return ctx.wait_until(probe, timeout, every=3, waiting=f"{name} is niet klaar geraakt")


def waited(ctx: Ctx, dag: str, run_id: str, timeout: int, after_clear: int = -1) -> tuple[str, list[dict]]:
    """`wait_for_finish`, but a timeout or a Cancel still reports what the started run changes."""
    try:
        return wait_for_finish(ctx, ctx.env, dag, run_id, timeout, after_clear)
    except (StepFailed, Cancelled) as e:
        e.touched = set(getattr(e, "touched", ())) | {f"models:{ctx.env}", "mail"}
        raise


def judge(dag: str, env: str, expect: str, state: str, tasks: list[dict], run_id: str) -> Result:
    name, where = upper_first(dag_name(dag)), clients.ENV_WORDS[env]
    stopped = failed_label(tasks, dag)
    details = f"run {run_id}: {state}; tasks: " + ", ".join(f"{t['task_id']}={t['state']}" for t in tasks)
    touched = {f"models:{env}", "mail"}
    if state == expect == "success":
        summary = f"{name} is zonder problemen afgelopen in {where}."
    elif state == expect == "failed":
        summary = f"{name} is mislukt in {where}, zoals verwacht" + (f", bij de stap '{stopped}'." if stopped else ".")
    elif state == "failed":
        raise StepFailed(f"{name} is mislukt in {where}" + (f" bij de stap '{stopped}'." if stopped else "."), details, touched)
    else:
        raise StepFailed(f"{name} had moeten mislukken in {where}, maar is zonder problemen afgelopen.", details, touched)
    return Result(summary, details=details, touched=touched, data={"state": state})   # the run id is only in the details


@action("airflow.run_dag", RunDagParams, lambda p: f"{upper_first(dag_name(p.dag))} starten")
def run_dag(ctx: Ctx, p: RunDagParams) -> Result:
    rest = {f"dag:{ctx.env}:{p.dag}"} if p.dag in PAUSED_AT_REST else set()   # started here, so switched on here
    try:
        run_id = trigger(ctx, ctx.env, p.dag, p.conf)
        state, tasks = waited(ctx, p.dag, run_id, p.timeout)
        result = judge(p.dag, ctx.env, p.expect, state, tasks, run_id)
    except (StepFailed, Cancelled) as e:
        e.touched = set(getattr(e, "touched", ())) | rest
        raise
    result.touched |= rest
    return result


@action("airflow.retry_failed", RetryFailedParams, lambda p: f"Retry van enkel het mislukte deel van {dag_name(p.dag)}")
def retry_failed(ctx: Ctx, p: RetryFailedParams) -> Result:
    run = latest_run(ctx, ctx.env, p.dag)
    if run is None or run["logical_date"] is None:
        raise StepFailed(
            f"Er is geen eerdere run van {dag_name(p.dag)} voor een retry.", f"{p.dag}: no dag_run with a logical date"
        )
    date = iso(run["logical_date"])
    clients.airflow_cli(ctx, ctx.env, "tasks", "clear", p.dag, "-f", "-y", "-s", date, "-e", date)
    state, tasks = waited(ctx, p.dag, run["run_id"], p.timeout, after_clear=run["clear_number"])
    return judge(p.dag, ctx.env, p.expect, state, tasks, run["run_id"])


# --- the scheduler's own switches -----------------------------------------------------------------------------
# Switching a job on, the settings that are read when a job's file is parsed, feeds that report in, runs that start by
# themselves. Everything is read from, or done through, the scheduler's own container and metadata database: there is
# no scheduler API user in the stack.

NIGHTLY = "dbt_daily_load"
PARSE_TIME_SETTINGS = ("dbt_dataset_dependencies", "dbt_emit_asset_events")   # read when dbt_daily_load.py is parsed
EMIT_TASKS = ("extract_refreshed_tables", "emit_asset_events")                # exist only while the second one is true
# The word a scenario uses for a feed -> the landing table that stands for it (names.TABLES has its plain name).
FEEDS = {
    "orders": "raw_olist_orders",
    "customers": "raw_olist_customers",
    "payments": "raw_olist_order_payments",
    "reviews": "raw_olist_order_reviews",
    "carrier": "raw_carrier_tracking_events",
    "web": "raw_web_events",
}
RUN_STARTERS = {"asset_triggered": "de feeds", "scheduled": "de klok", "manual": "een persoon"}
# What an asset event is, per job, when a line says "N seconds after ..."
EVENT_WORDS = {NIGHTLY: "de laatste feed zich had gemeld", "report_refreshed_notice": "het managementrapport gemeld werd"}
# The scheduler's container runs this; it is what the REST call `POST /assets/events` does, without a login. The
# URIs come in as one JSON list, the `extra` of every event as the second argument.
EMIT_SCRIPT = """
import json, sys
from sqlalchemy import select
from airflow.assets.manager import asset_manager
from airflow.models.asset import AssetModel
from airflow.utils.session import create_session
from airflow._shared.timezones import timezone
uris, extra = json.loads(sys.argv[1]), json.loads(sys.argv[2])
out = []
with create_session() as session:
    for uri in uris:
        model = session.scalar(select(AssetModel).where(AssetModel.uri == uri).limit(1))
        if model is None:
            print(json.dumps({"error": "unknown asset", "uri": uri})); raise SystemExit(3)
        event = asset_manager.register_asset_change(
            asset=model, timestamp=timezone.utcnow(), extra=extra, source_is_api=True, session=session)
        out.append(getattr(event, "id", None))
print(json.dumps({"events": out}))
"""


def asset_uri(env: str, database: str, name: str) -> str:
    """`s2://workshop-<env>/<database>/<object>`, as utils/dbt_asset_utils.py of the dbt project writes it."""
    return f"s2://workshop-{env}/{database}/{name}"


def feed_uri(env: str, feed: str) -> str:
    return asset_uri(env, f"{env}_landing", FEEDS[feed])


def feed_words(uri: str) -> str:
    """"Orders feed" for the URI of a landing table; the name of the table when there is no plain word for it."""
    table = uri.rsplit("/", 1)[-1]
    return names.TABLES.get(table) or names.model_title(table) or table


def task_ids(node: Any) -> set[str]:
    """Every `task_id` somewhere in a serialized DAG."""
    found: set[str] = set()
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "task_id" and isinstance(value, str):
                found.add(value)
            else:
                found |= task_ids(value)
    elif isinstance(node, list):
        for value in node:
            found |= task_ids(value)
    return found


def dag_shape(ctx: Ctx, env: str, dag: str) -> tuple[str, set[str]] | None:
    """How the scheduler has read the DAG file now: ("assets" or "clock", its task ids); None when it has not read it."""
    head = clients.airflow_query(ctx.settings, env, "SELECT timetable_summary FROM dag WHERE dag_id = %s", (dag,))
    stored = clients.airflow_query(
        ctx.settings, env, "SELECT data FROM serialized_dag WHERE dag_id = %s ORDER BY last_updated DESC LIMIT 1", (dag,)
    )
    if not head or not stored:
        return None
    data = stored[0]["data"]
    if isinstance(data, (str, bytes)):
        data = json.loads(data)
    return ("assets" if str(head[0]["timetable_summary"]) == "Asset" else "clock"), task_ids(data)


def wait_for_shape(
    ctx: Ctx, env: str, dag: str, schedule: str | None = None, has: tuple[str, ...] = (), lacks: tuple[str, ...] = (),
    timeout: int = 120, missing_ok: bool = False,
) -> tuple[str, set[str]] | None:
    """Poll until the scheduler's reading of the DAG file has `schedule` and the tasks (`has` present, `lacks` absent).
    The file is read again about every 30 seconds, so a setting that is read at parse time takes up to that long."""

    def probe() -> Any:
        shape = dag_shape(ctx, env, dag)
        if shape is None:
            return True if missing_ok else None
        kind, tasks = shape
        if (schedule is None or kind == schedule) and set(has) <= tasks and not set(lacks) & tasks:
            return shape
        ctx.progress("Wachten tot de scheduler de nieuwe instelling inleest")
        return None

    found = ctx.wait_until(probe, timeout, every=2, waiting=f"{dag_name(dag)} heeft de nieuwe instelling niet ingelezen")
    return None if found is True else found


class WaitForDagParams(Params):
    dag: str = NIGHTLY
    schedule: Literal["assets", "clock"] | None = None
    has_tasks: list[str] = Field(default_factory=list)
    lacks_tasks: list[str] = Field(default_factory=list)
    timeout: int = 120


@action("airflow.wait_for_dag", WaitForDagParams, lambda p: "Wachten tot de scheduler de nieuwe instelling inleest")
def wait_for_dag(ctx: Ctx, p: WaitForDagParams) -> Result:
    kind, tasks = wait_for_shape(ctx, ctx.env, p.dag, p.schedule, tuple(p.has_tasks), tuple(p.lacks_tasks), p.timeout)
    name = dag_name(p.dag)
    parts = ["wacht nu op de feeds in plaats van op een vast uur" if kind == "assets" else "draait op een vast uur"]
    announces = set(EMIT_TASKS) <= tasks
    parts.append("meldt welke tabellen ververst zijn" if announces else "meldt niets")
    return Result(
        f"De scheduler heeft de nieuwe instelling ingelezen: {name} {' en '.join(parts)}.",
        details=f"{p.dag}: {kind} schedule; tasks: {', '.join(sorted(tasks))}",
        data={"schedule": kind, "announces": announces},
    )


class DagParams(Params):
    dag: str


def run_mark(ctx: Ctx, env: str, dag: str) -> int:
    """The newest run of the DAG so far: a run with a higher number started after this moment."""
    rows = clients.airflow_query(ctx.settings, env, "SELECT COALESCE(MAX(id), 0) AS mark FROM dag_run WHERE dag_id = %s", (dag,))
    return int(rows[0]["mark"]) if rows else 0


@action("airflow.unpause_dag", DagParams, lambda p: f"{upper_first(dag_name(p.dag))} aanzetten")
def unpause_dag(ctx: Ctx, p: DagParams) -> Result:
    clients.airflow_cli(ctx, ctx.env, "dags", "unpause", p.dag)
    return Result(
        f"{upper_first(dag_name(p.dag))} is aangezet in {clients.ENV_WORDS[ctx.env]}.",
        details=f"airflow dags unpause {p.dag}",
        touched={f"dag:{ctx.env}:{p.dag}"},
        data={"run_mark": run_mark(ctx, ctx.env, p.dag)},
    )


@cleaner("dag")
def clean_dag(ctx: Ctx, resource: str) -> Result:
    """Pauses the DAG again. Every DAG is created paused, and a run started by hand switches it on by itself."""
    _, env, dag = resource.split(":")
    clients.airflow_cli(ctx, env, "dags", "pause", dag, check=False)
    return Result(f"{upper_first(dag_name(dag))} is opnieuw uitgezet in {clients.ENV_WORDS[env]}.")


class ReportFeedsParams(Params):
    feeds: list[Literal["orders", "customers", "payments", "reviews", "carrier", "web"]] = Field(min_length=1)
    dag: str = NIGHTLY


def post_events(ctx: Ctx, env: str, uris: list[str]) -> list[Any]:
    """One asset event per URI, in the order given. A feed that is not a known asset fails the step."""
    proc = clients.docker(
        ctx, "exec", f"{ctx.settings.airflow_container_prefix}{env}", "python3", "-c", EMIT_SCRIPT,
        json.dumps(uris), json.dumps({"reported_by": "workshop studio"}), timeout=120, check=False,
    )
    last = next((ln for ln in reversed(proc.stdout.splitlines()) if ln.startswith("{")), "")
    try:
        answer = json.loads(last)
    except json.JSONDecodeError:
        answer = {}
    if proc.returncode != 0 or "events" not in answer:
        what = f" ({answer['uri'].rsplit('/', 1)[-1]})" if answer.get("uri") else ""
        raise StepFailed(
            f"De scheduler kent deze feed nog niet{what}: de nachtelijke load moet er eerst op wachten.",
            proc.text[-600:],
        )
    return answer["events"]


@action("airflow.report_feeds", ReportFeedsParams, lambda p: "De feeds laten melden dat ze binnen zijn")
def report_feeds(ctx: Ctx, p: ReportFeedsParams) -> Result:
    """Each feed in turn tells the scheduler that new data has landed (an event on the feed's asset). The lines say what
    the nightly load still waits for after each one. A job that is switched off would miss the event, so that fails."""
    env, name = ctx.env, upper_first(dag_name(p.dag))
    state = clients.airflow_query(ctx.settings, env, "SELECT is_paused FROM dag WHERE dag_id = %s", (p.dag,))
    needed = [r["uri"] for r in clients.airflow_query(
        ctx.settings, env,
        "SELECT a.uri FROM dag_schedule_asset_reference r JOIN asset a ON a.id = r.asset_id WHERE r.dag_id = %s", (p.dag,))]
    queued = [r["uri"] for r in clients.airflow_query(
        ctx.settings, env,
        "SELECT a.uri FROM asset_dag_run_queue q JOIN asset a ON a.id = q.asset_id WHERE q.target_dag_id = %s", (p.dag,))]
    if not needed:
        raise StepFailed(f"{name} wacht nog niet op feeds.", f"{p.dag} has no asset in its schedule")
    if not state or state[0]["is_paused"]:
        raise StepFailed(f"{name} staat uit en merkt de feeds dus niet op.", f"{p.dag} is paused")
    mark = run_mark(ctx, env, p.dag)
    uris = [feed_uri(env, feed) for feed in p.feeds]
    ids = post_events(ctx, env, uris)
    arrived, reasons, waiting = set(queued), [], list(needed)
    for uri in uris:
        again = uri in arrived
        arrived.add(uri)
        waiting = [u for u in needed if u not in arrived]
        report = f"{feed_words(uri)}: {'opnieuw ' if again else ''}gemeld."
        reasons.append(
            f"{report} {name} wacht nog op: {', '.join(feed_words(u) for u in waiting)}."
            if waiting else f"{report} Elke feed waarop {dag_name(p.dag)} wacht, heeft zich nu gemeld."
        )
    summary = (f"De feeds hebben zich gemeld; {dag_name(p.dag)} wacht nog op {plural(len(waiting), 'feed', 'feeds')}." if waiting
               else f"De feeds hebben zich gemeld; elke feed waarop {dag_name(p.dag)} wacht, heeft zich nu gemeld.")
    return Result(
        summary,
        details=f"asset events {ids}: {', '.join(uris)}",
        data={"reasons": reasons, "run_mark": mark, "waiting": [feed_words(u) for u in waiting], "all_in": not waiting},
    )


class WaitForRunParams(Params):
    dag: str
    after: str   # id of an earlier action of the step whose result carries `run_mark` (report_feeds, unpause_dag)
    expect: Literal["success", "failed"] = "success"
    timeout: int = 300


@action("airflow.wait_for_run", WaitForRunParams, lambda p: f"Wachten tot {dag_name(p.dag)} vanzelf start")
def wait_for_run(ctx: Ctx, p: WaitForRunParams) -> Result:
    """Waits for the first run of the DAG that began after the marked moment, whatever started it, and for its end."""
    earlier = ctx.results.get(p.after)
    mark = (earlier.data if earlier else {}).get("run_mark")
    if mark is None:
        raise StepFailed("Een stap wacht op een run die vanzelf start, maar niets geeft aan vanaf wanneer er gekeken moet worden.",
                         f"no run_mark in the result of the action '{p.after}'")
    env, name = ctx.env, dag_name(p.dag)

    def started() -> Any:
        rows = clients.airflow_query(
            ctx.settings, env,
            "SELECT id, run_id, run_type, queued_at FROM dag_run WHERE dag_id = %s AND id > %s ORDER BY id LIMIT 1",
            (p.dag, mark),
        )
        if not rows:
            ctx.progress(f"Wachten tot {name} vanzelf start")
        return rows[0] if rows else None

    run = ctx.wait_until(started, 120, every=2, waiting=f"{name} is niet vanzelf gestart")
    state, tasks = wait_for_finish(ctx, env, p.dag, run["run_id"], p.timeout)
    result = judge(p.dag, env, p.expect, state, tasks, run["run_id"])
    starter = RUN_STARTERS.get(run["run_type"], "iets anders")
    reasons = [f"{upper_first(name)} is vanzelf gestart, op gang gebracht door {starter}."]
    if run["run_type"] == "asset_triggered":
        last = clients.airflow_query(
            ctx.settings, env,
            "SELECT MAX(e.timestamp) AS at FROM dagrun_asset_event d JOIN asset_event e ON e.id = d.event_id "
            "WHERE d.dag_run_id = %s", (run["id"],))
        if last and isinstance(last[0]["at"], datetime) and isinstance(run["queued_at"], datetime):
            after = max(0.0, (run["queued_at"] - last[0]["at"]).total_seconds())
            delay = "minder dan een seconde" if after < 1 else plural(round(after), "seconde", "seconden")
            reasons[0] = (f"{upper_first(name)} is vanzelf gestart, {delay} nadat "
                          f"{EVENT_WORDS.get(p.dag, 'de laatste melding binnenkwam')}.")
        reasons.append("Geen persoon en geen klok heeft dit in gang gezet.")
    if p.dag in PAUSED_AT_REST:   # a notice builds nothing: only the inbox and the switch change
        result.touched = {"mail", f"dag:{env}:{p.dag}"}
    result.data = {**result.data, "reasons": reasons, "started_by": starter}
    result.summary += " Dat gebeurde vanzelf." if run["run_type"] == "asset_triggered" else ""
    return result


class AnnouncementsParams(Params):
    dag: str = NIGHTLY
    tables: list[str] = Field(default_factory=list)   # tables whose announcement is asked about (their dbt names)


@action("airflow.announcements", AnnouncementsParams, lambda p: f"Nakijken wat {dag_name(p.dag)} gemeld heeft")
def announcements(ctx: Ctx, p: AnnouncementsParams) -> Result:
    """What the newest run of the DAG announced: how many tables, and whether each table asked about was among them.
    Read-only."""
    run = latest_run(ctx, ctx.env, p.dag)
    if run is None:
        raise StepFailed(f"Er is geen eerdere run van {dag_name(p.dag)}.", f"{p.dag}: no dag_run")
    rows = clients.airflow_query(
        ctx.settings, ctx.env,
        "SELECT a.uri FROM asset_event e JOIN asset a ON a.id = e.asset_id WHERE e.source_dag_id = %s AND e.source_run_id = %s",
        (p.dag, run["run_id"]),
    )
    announced = {r["uri"].rsplit("/", 1)[-1] for r in rows}
    name = upper_first(dag_name(p.dag))
    reasons = [f"{name} meldde {plural(len(announced), 'ververste tabel', 'ververste tabellen')}."
               if announced else f"{name} meldde geen enkele tabel."]
    asked = {}
    for table in p.tables:
        asked[table] = table in announced
        title = (names.TABLES.get(table) or names.model_title(table) or table)
        reasons.append(f"{upper_first(title)}: {'wel' if table in announced else 'niet'} gemeld.")
    return Result(
        f"De laatste run meldde {plural(len(announced), 'tabel', 'tabellen')}.",
        details=f"run {run['run_id']}: {sorted(announced)}",
        data={"reasons": reasons, "announced": len(announced), "tables": asked},
    )
