"""Evidence providers: what the audience looks at (`show:` items of a scenario).

Each provider returns a small JSON-serialisable dict for a UI that is read by non-technical people: plain labels,
states as a code (`good|bad|neutral`, `done|running|failed|todo|skipped`, `up|down|starting`) next to words, never raw
ids. SQL evidence is read-only, runs as root, has a 5 second limit and returns at most 200 rows.
"""

from __future__ import annotations

import html
import json
import re
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta, timezone
from decimal import Decimal
from pathlib import Path
from typing import Any

import pymysql
import requests
from pydantic import BaseModel, ConfigDict, Field, field_validator

from .actions import airflow, clients
from .actions.base import Ctx, Params, evidence
from .actions.gitlab import (failures, find_pipeline, job_word, ordered_jobs, recorded_tags, rehearsed, settled,
                             tables_in_project)
from .names import MONTHS, SHORT_MONTHS, format_number, plain_cells, plain_pair

MAX_ROWS = 200
MISSING_TABLE = (1146, 1049)   # no such table / no such database
RUN_WORDS = {
    "success": ("done", "Klaar"),
    "failed": ("failed", "Mislukt"),
    "running": ("running", "Bezig"),
    "queued": ("todo", "Wacht"),
}
JOB_STATES = {
    "success": ("done", "Klaar"),
    "failed": ("failed", "Mislukt"),
    "running": ("running", "Bezig"),
    "pending": ("todo", "Wacht"),
    "created": ("todo", "Wacht"),
    "waiting_for_resource": ("todo", "Wacht"),
    "manual": ("todo", "Wacht op een persoon"),
    "skipped": ("skipped", "Overgeslagen"),
    "canceled": ("skipped", "Geannuleerd"),
}
FRESHNESS_STATES = {
    "pass": ("fresh", "Up-to-date"),
    "warn": ("late", "Laat"),
    "error": ("very_late", "Heel laat"),
    "runtime error": ("unknown", "Kon niet nagekeken worden"),
}


class Show(Params):
    title: str = ""
    caption: str = ""


class Column(BaseModel):
    model_config = ConfigDict(extra="forbid")
    key: str
    label: str
    unit: str | None = None   # what the numbers of the column are counted in ("seconds"); the UI puts it in the header


class Highlight(BaseModel):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)
    column: str
    equals: Any
    as_: str = Field("good", alias="as", pattern="^(good|bad)$")


class Compare(BaseModel):
    model_config = ConfigDict(extra="forbid")
    query: str
    label: str


class TableParams(Show):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)
    query: str
    as_: str = Field("root", alias="as")   # the database user that reads the rows
    columns: list[Column] | None = None
    highlight: Highlight | None = None
    empty: str = "Er is nog niets te zien."
    footer: str | None = None   # one line under the table ("De 9 traagste van 28 tabellen en 21 checks")


class BarsParams(Show):
    query: str  # returns label, series, value (or just label, value for a single series)
    good: list[str] = Field(default_factory=list)      # series names; a single name may be written as plain text
    bad: list[str] = Field(default_factory=list)
    neutral: list[str] = Field(default_factory=list)
    x_label: str = ""
    unit: str | None = None   # plural noun of what is counted ("pakketscans"); default: guessed from the tables read

    @field_validator("good", "bad", "neutral", mode="before")
    @classmethod
    def one_name_is_a_list(cls, value: Any) -> Any:
        return [value] if isinstance(value, str) else value


class NumberParams(Show):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)
    query: str
    as_: str = Field("root", alias="as")   # the database user that reads the number
    unit: str | None = None   # default: guessed from the tables read
    compare_to: Compare | None = None


class TasksParams(Show):
    dag: str


class PipelineParams(Show):
    for_: str = Field(alias="for")
    rehearsed: bool = False   # also return how many tables and checks the rehearsal covered (for the card to draw)
    model_config = ConfigDict(extra="forbid", populate_by_name=True)


class MailParams(Show):
    match: str


class RunHistoryParams(Show):
    limit: int = Field(5, ge=1, le=20)


# What a query counts, from the tables it reads (first match wins; "payments" before "orders").
UNIT_BY_TABLE = (
    (r"carrier|tracking_event", "pakketscans"), (r"web_|website", "websitebezoeken"),
    (r"payment", "betalingen"), (r"review", "reviews"), (r"deliver", "leveringen"),
    (r"customer", "klanten"), (r"order", "bestellingen"),
)


def unit_for(query: str, given: str | None = None) -> str | None:
    """The unit word of a chart or number: the scenario's own `unit:` when it gives one, otherwise a plural noun
    guessed from the tables in the query, otherwise None."""
    if given:
        return given
    tables = " ".join(re.findall(r"(?i)\b(?:from|join)\s+`?([\w.]+)", query)).lower()
    return next((unit for pattern, unit in UNIT_BY_TABLE if re.search(pattern, tables)), None)


def plain(value: Any) -> Any:
    """JSON-safe cell: decimals become numbers, dates become ISO text."""
    if isinstance(value, Decimal):
        return int(value) if value == value.to_integral_value() else float(value)
    if isinstance(value, (datetime, date)):
        return value.isoformat()
    if isinstance(value, (bytes, bytearray)):
        return value.decode(errors="replace")
    return value


def rows_of(ctx: Ctx, query: str, user: str = "root") -> tuple[list[str], list[list[Any]]]:
    names, rows = clients.read_rows(ctx.settings, query, limit=MAX_ROWS, user=user)
    return names, [[plain(v) for v in row] for row in rows]


# --- SQL evidence ---------------------------------------------------------------------------------


@evidence("table", TableParams)
def table(ctx: Ctx, p: TableParams) -> dict[str, Any]:
    names, rows = rows_of(ctx, p.query, p.as_)
    columns = [c.model_dump(exclude_none=True) for c in p.columns] if p.columns else [{"key": n, "label": n} for n in names]
    out = []
    for row in rows:
        raw = dict(zip(names, row))
        cells, technical = plain_cells(raw)   # check, account and database names are shown in plain words
        state = None
        if p.highlight and raw.get(p.highlight.column) == p.highlight.equals:
            state = p.highlight.as_
        out.append({"cells": cells, "state": state, **({"details": "; ".join(technical)} if technical else {})})
    return {"columns": columns, "rows": out, "empty": p.empty, "truncated": len(rows) >= MAX_ROWS, "footer": p.footer}


def label_day(label: Any) -> date | None:
    """The calendar day a chart label stands for (`2026-11-15`, `2026-11-15T04:00:00`), or None."""
    try:
        return date.fromisoformat(label[:10]) if isinstance(label, str) else None
    except ValueError:
        return None


def today_of(ctx: Ctx) -> date:
    try:
        return ctx.feeder.today(ctx.env)
    except Exception:  # noqa: BLE001 - the real date is what the feeder uses anyway
        return date.today()


@evidence("bars", BarsParams)
def bars(ctx: Ctx, p: BarsParams) -> dict[str, Any]:
    _, rows = rows_of(ctx, p.query)
    labels: list[Any] = []
    values: dict[str, dict[Any, float]] = {}
    for row in rows:
        label, series, value = (row[0], "", row[1]) if len(row) == 2 else row[:3]
        if label not in labels:
            labels.append(label)
        values.setdefault(str(series), {})[label] = value or 0
    days = [label_day(label) for label in labels]
    if labels and all(days):   # oldest first, today last, whatever order the query gave
        labels = [label for _, label in sorted(zip(days, labels), key=lambda pair: pair[0])]
        days = sorted(days)
    today = today_of(ctx)
    bars = []
    for label, day in zip(labels, days):
        text = f"{day.day} {SHORT_MONTHS[day.month - 1]}" if day else str(label)
        bars.append({"key": label, "label": f"{text} (vandaag)" if day == today else text, "today": day == today})
    state = lambda name: "good" if name in p.good else "bad" if name in p.bad else "neutral"  # noqa: E731
    return {
        "labels": labels,
        "bars": bars,
        "x_label": p.x_label,
        "unit": unit_for(p.query, p.unit),
        "series": [
            {"name": name, "state": state(name), "values": [by_label.get(label, 0) for label in labels]}
            for name, by_label in values.items()
        ],
    }


@evidence("number", NumberParams)
def number(ctx: Ctx, p: NumberParams) -> dict[str, Any]:
    def first(query: str, patient: bool = False) -> Any:
        for attempt in range(4):
            try:
                _, rows = rows_of(ctx, query, p.as_)
            except pymysql.MySQLError as exc:
                if exc.args and exc.args[0] in MISSING_TABLE:   # the table is not there (yet): nothing counted, so 0
                    if patient and attempt < 3:   # a table being swapped is missing for a fraction of a second
                        time.sleep(0.3)
                        continue
                    return 0
                raise
            return rows[0][0] if rows else None

    value = first(p.query, patient=True)
    compare = {"label": p.compare_to.label, "value": first(p.compare_to.query)} if p.compare_to else None
    return {"value": value, "unit": unit_for(p.query, p.unit), "compare": compare}


# --- Airflow, GitLab, Mailpit ---------------------------------------------------------------------


@evidence("tasks", TasksParams)
def tasks(ctx: Ctx, p: TasksParams) -> dict[str, Any]:
    run = airflow.latest_run(ctx, ctx.env, p.dag)
    if run is None:
        return {"title": airflow.dag_name(p.dag), "run": None, "tasks": [], "empty": "Er is nog niets gerund."}
    rows = airflow.task_rows(ctx, ctx.env, p.dag, run["run_id"])
    state, words = RUN_WORDS.get(airflow.run_state(p.dag, run, rows), ("todo", "Wacht"))
    return {
        "title": airflow.dag_name(p.dag),
        "run": {
            "state": state,
            "state_text": words,
            "started": airflow.iso(run["start_date"]),
            "finished": airflow.iso(run["end_date"]),
        },
        "tasks": airflow.describe_tasks(rows, run, p.dag),
    }


@evidence("pipeline", PipelineParams)
def pipeline(ctx: Ctx, p: PipelineParams) -> dict[str, Any]:
    api = clients.GitLabApi(ctx.settings)
    found = find_pipeline(api, p.for_, recorded_tags(ctx.settings))
    if found is None:
        return {"state": "todo", "state_text": "Nog niet gestart", "jobs": [], "failures": [], **({"rehearsed": None} if p.rehearsed else {})}
    state, words = JOB_STATES.get(settled(api, found), ("running", "Bezig"))
    raw_jobs = ordered_jobs(api.jobs(found["id"]))
    jobs = [
        {
            "label": job_word(j["name"]),
            "state": JOB_STATES.get(j["status"], ("todo", "Wacht"))[0],
            "state_text": JOB_STATES.get(j["status"], ("todo", "Wacht"))[1],
        }
        for j in raw_jobs
    ]
    return {"state": state, "state_text": words, "jobs": jobs,
            "failures": failures(api, raw_jobs) if state == "failed" else [],
            **({"rehearsed": rehearsed(ctx.settings, api, raw_jobs)} if p.rehearsed else {})}


def mailpit(ctx: Ctx, path: str, **params: str) -> dict[str, Any]:
    r = requests.get(f"{ctx.settings.mailpit_url}/api/v1/{path}", params=params, timeout=5)
    r.raise_for_status()
    return r.json()


@evidence("mail", MailParams)
def mail(ctx: Ctx, p: MailParams) -> dict[str, Any]:
    found = mailpit(ctx, "search", query=f'subject:"{p.match}"', limit="1")["messages"]
    if not found:
        return {"found": False, "empty": "Er is nog geen mail binnengekomen."}
    message = mailpit(ctx, f"message/{found[0]['ID']}")
    sender = message.get("From") or {}
    body = message.get("HTML") or f"<pre>{html.escape(message.get('Text') or '')}</pre>"
    return {
        "found": True,
        "subject": message.get("Subject", ""),
        "from": sender.get("Name") or sender.get("Address", ""),
        "received": message.get("Date") or found[0].get("Created"),
        "html": body,
    }


# --- versions, the step's own result, freshness, history ------------------------------------------


class VersionsParams(Show):
    only: str = ""   # read just this environment (one call instead of three)


@evidence("versions", VersionsParams)
def versions(ctx: Ctx, p: VersionsParams) -> dict[str, Any]:
    """The deployed version per environment. Airflow stores its settings encrypted, so they are read with the CLI
    (one call per environment, all at the same time: one call takes about a second)."""
    only = getattr(p, "only", "")
    envs = (only,) if only in clients.ENVS else clients.ENVS

    def tag_of(env: str) -> str | None:
        try:
            return clients.deployed_tag(ctx, env)
        except Exception:  # noqa: BLE001 - an environment that cannot be asked shows as unknown, not as an error
            return None

    with ThreadPoolExecutor(max_workers=len(envs)) as pool:
        tags = list(pool.map(tag_of, envs))
    return {
        "environments": [
            {"env": env, "label": clients.ENV_WORDS[env].capitalize(), "tag": tag}
            for env, tag in zip(envs, tags)
        ]
    }


class ResultParams(Show):
    from_action: str = ""   # show the result of this action (e.g. git.revert_on_release) instead of the last one


@evidence("result", ResultParams)
def result(ctx: Ctx, p: ResultParams) -> dict[str, Any]:
    last = ctx.results.get(p.from_action) if p.from_action else ctx.last_result
    summary, details = plain_pair(last.summary, last.details) if last else ("", "")
    return {"summary": summary, "details": details, "data": last.data if last else {}}


def age_words(seconds: float | None) -> str:
    if seconds is None:
        return "nooit"
    if seconds < 90:
        return "zonet"
    for size, unit, many in ((86400, "dag", "dagen"), (3600, "uur", "uur"), (60, "minuut", "minuten")):
        if seconds >= size:
            count = int(seconds // size)
            return f"{count} {unit if count == 1 else many} geleden"
    return "zonet"


SOURCE_WORDS = {
    "raw_olist_customers": "klantgegevens",
    "raw_olist_orders": "bestellingen",
    "raw_olist_order_payments": "betalingen",
    "raw_olist_order_reviews": "reviews",
    "raw_carrier_tracking_events": "pakketscans",
    "raw_web_events": "websitebezoeken",
}


def source_label(unique_id: str) -> str:
    """`source.pkg.olist.raw_olist_orders` -> `bestellingen` (a data feed in everyday words)."""
    name = unique_id.split(".")[-1]
    return SOURCE_WORDS.get(name, name.removeprefix("raw_").removeprefix("olist_").replace("_", " "))


@evidence("freshness", Show)
def freshness(ctx: Ctx, p: Show) -> dict[str, Any]:
    path = Path(ctx.settings.state_dir) / ctx.env / "mnt_dbt" / "target_freshness" / "sources.json"
    if not path.exists():
        return {"sources": [], "empty": "De feeds zijn nog niet nagekeken."}
    report = json.loads(path.read_text())
    sources = []
    for r in report.get("results", []):
        state, words = FRESHNESS_STATES.get(str(r.get("status", "")).lower(), ("unknown", "Kon niet nagekeken worden"))
        sources.append(
            {
                "label": source_label(r.get("unique_id", "")),
                "state": state,
                "state_text": words,
                "last_data": age_words(r.get("max_loaded_at_time_ago_in_s")),
            }
        )
    return {"generated_at": (report.get("metadata") or {}).get("generated_at"), "sources": sources}


def plural(n: int, one: str, many: str | None = None) -> str:
    """"1 tabel", "2 tabellen": pass the plural form, Dutch plurals are not regular (default: one + "en")."""
    return f"{format_number(n)} {one if n == 1 else many or one + 'en'}"


def run_summary(run: dict[str, Any]) -> str:
    """The sentence of one run from its numbers (the keys of a `run_history` run): "28 van 28 tabellen gebuild, 21
    checks geslaagd", "24 van 28 tabellen gebuild, 4 tabellen en 2 checks niet gerund, 1 check mislukt", "25 van 28
    tabellen gebuild, 3 bewust overgeslagen, 14 checks geslaagd", "..., 20 checks geslaagd, 1 check met waarschuwing", "Retry:
    4 tabellen gebuild, 3 checks geslaagd"."""
    built, total, left_out = run["tables_built"], run["tables_total"], run["tables_left_out"]
    not_run_tables = run["tables_not_run"]
    problems = bool(run["tables_failed"] or run["checks_failed"])
    table, check = ("tabel", "tabellen"), ("check", "checks")
    if run["retry"]:
        parts = [plural(built, *table) + " gebuild" if built == total else f"{format_number(built)} van {format_number(total)} tabellen gebuild"]
    else:
        parts = [f"{format_number(built)} van {format_number(total + left_out)} tabellen gebuild"]
        if left_out:
            parts.append(f"{format_number(left_out)} bewust overgeslagen")
    if not_run_tables or run["checks_not_run"]:
        what = [plural(n, *word) for n, word in ((not_run_tables, table), (run["checks_not_run"], check)) if n]
        parts.append(" en ".join(what) + " niet gerund")
    if problems:
        what = [plural(n, *word) for n, word in ((run["tables_failed"], table), (run["checks_failed"], check)) if n]
        parts.append(" en ".join(what) + " mislukt")
    elif not not_run_tables and not run["checks_not_run"] and run["checks_passed"]:
        parts.append(f"{plural(run['checks_passed'], *check)} geslaagd")
    if run["checks_warned"]:
        parts.append(f"{plural(run['checks_warned'], *check)} met waarschuwing")
    return ("Retry: " if run["retry"] else "") + ", ".join(parts)


def run_state(run: dict[str, Any]) -> str:
    """`bad` with any failure, `warn` with only warnings, otherwise `good`."""
    if run["tables_failed"] or run["checks_failed"]:
        return "bad"
    return "warn" if run["checks_warned"] else "good"


def is_retry(run: dict[str, Any], older: dict[str, Any] | None) -> bool:
    """A run that only redid what the run before it left undone: that one had problems, and this one is smaller
    and fits in the tables that were not built."""
    if older is None or not (older["tables_failed"] or older["checks_failed"] or older["tables_total"] > older["tables_built"]
                             or older["checks_not_run"]):
        return False
    undone = older["tables_total"] - older["tables_built"]
    size = lambda r: r["tables_total"] + r["checks_passed"] + r["checks_failed"] + r["checks_warned"] + r["checks_not_run"]  # noqa: E731
    return run["tables_total"] <= undone and size(run) < size(older)


@evidence("run_history", RunHistoryParams)
def run_history(ctx: Ctx, p: RunHistoryParams) -> dict[str, Any]:
    is_check = "resource_type IN ('test', 'unit_test')"
    query = (
        f"SELECT MIN(run_started_at), "
        f"SUM(resource_type = 'model' AND status IN ('success', 'pass')), "
        f"SUM(resource_type = 'model' AND status = 'skipped'), "
        f"SUM(resource_type = 'model' AND status IN ('error', 'fail')), "
        f"SUM({is_check} AND status IN ('pass', 'success')), "
        f"SUM({is_check} AND status = 'skipped'), "
        f"SUM({is_check} AND status IN ('error', 'fail')), "
        f"SUM({is_check} AND status = 'warn') FROM {ctx.env}_vdp_mgmt.dbt_run_results "
        f"GROUP BY invocation_id ORDER BY 1 DESC LIMIT {p.limit + 1}"   # one more: the run before the oldest decides "retry"
    )
    _, rows = rows_of(ctx, query)
    rows = sorted(rows, key=lambda r: str(r[0] or ""), reverse=True)   # newest first, whatever the order of the rows
    try:
        project_total = tables_in_project(ctx.settings)
    except AttributeError:   # no settings: the project's size is unknown, so nothing counts as left out
        project_total = None
    runs = []
    for started, built, tables_skipped, tables_failed, passed, checks_skipped, checks_failed, warned in rows:
        built, tables_skipped, tables_failed = int(built or 0), int(tables_skipped or 0), int(tables_failed or 0)
        total = built + tables_skipped + tables_failed
        runs.append({
            "started": started,
            "tables_built": built, "tables_total": total, "tables_not_run": tables_skipped,
            "tables_failed": tables_failed, "tables_left_out": max(0, project_total - total) if project_total else 0,
            "checks_passed": int(passed or 0), "checks_failed": int(checks_failed or 0),
            "checks_warned": int(warned or 0), "checks_not_run": int(checks_skipped or 0), "retry": False,
        })
    for run, older in zip(runs, runs[1:] + [None]):
        run["retry"] = is_retry(run, older)
        if run["retry"]:
            run["tables_left_out"] = 0   # a retry redoes part of a night; the rest is not "left out"
        run["state"] = run_state(run)
        run["summary"] = run_summary(run)
    return {"runs": runs[:p.limit], "empty": "Er is nog geen run vastgelegd."}


# --- stack status ---------------------------------------------------------------------------------


def probe(check: Any) -> tuple[str, str]:
    try:
        return check()
    except Exception as exc:  # noqa: BLE001 - every failure is a status, never an error for the audience
        return "down", f"Niet bereikbaar ({type(exc).__name__})"


def check_gitlab(ctx: Ctx) -> tuple[str, str]:
    r = requests.get(
        f"{ctx.settings.gitlab_url}/api/v4/version", headers={"PRIVATE-TOKEN": ctx.settings.gitlab_pat}, timeout=5
    )
    if r.status_code == 200:
        return "up", "De codeserver is klaar"
    return (
        ("starting", "De codeserver is nog aan het opstarten")
        if r.status_code >= 500
        else ("down", "De codeserver weigert de toegang")
    )


def check_singlestore(ctx: Ctx) -> tuple[str, str]:
    clients.read_rows(ctx.settings, "SELECT 1")
    return "up", "De bedrijfsdatabase is klaar"


def check_airflow(ctx: Ctx, env: str) -> tuple[str, str]:
    state = clients.docker(
        ctx,
        "inspect",
        "-f",
        "{{.State.Status}}",
        f"{ctx.settings.airflow_container_prefix}{env}",
        timeout=15,
        check=False,
    )
    if state.stdout.strip() != "running":
        return "down", "De scheduler draait niet"
    beat = clients.airflow_query(
        ctx.settings, env, "SELECT MAX(latest_heartbeat) AS beat FROM job WHERE job_type = 'SchedulerJob'"
    )[0]["beat"]
    fresh = beat is not None and datetime.now(timezone.utc) - beat.replace(tzinfo=timezone.utc) < timedelta(seconds=90)
    return ("up", "De scheduler is klaar") if fresh else ("starting", "De scheduler start op")


def check_mailpit(ctx: Ctx) -> tuple[str, str]:
    mailpit(ctx, "info")
    return "up", "De testmailbox is klaar"


def check_feeder(ctx: Ctx) -> tuple[str, str]:
    today = ctx.feeder.today(ctx.env)
    return "up", f"Levert data; vandaag is het {today.day} {MONTHS[today.month - 1]} {today.year}"


@evidence("stack", Show)
def stack(ctx: Ctx, p: Show) -> dict[str, Any]:
    checks = [
        ("gitlab", "Codeserver (GitLab)", lambda: check_gitlab(ctx)),
        ("singlestore", "Bedrijfsdatabase", lambda: check_singlestore(ctx)),
        *[
            (f"airflow-{env}", f"Nachtelijke load: {clients.ENV_WORDS[env]}", lambda e=env: check_airflow(ctx, e))
            for env in clients.ENVS
        ],
        ("mailpit", "Testmailbox", lambda: check_mailpit(ctx)),
        ("feeder", "Datalevering", lambda: check_feeder(ctx)),
    ]
    services = []
    for ident, label, check in checks:
        status, detail = probe(check)
        services.append({"id": ident, "label": label, "status": status, "detail": detail})
    return {"services": services}
