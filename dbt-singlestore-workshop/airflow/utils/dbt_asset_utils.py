"""Airflow Assets of the workshop: the URI of a table, the Variables that switch the asset features on, the events
emitted after a build and the plain-text notice sent when the management report was refreshed.

Pure Python (the Airflow `Asset` class is imported inside `make_asset`), so tests/test_asset_utils.py runs on a host
without Airflow. The DAGs that use it: dbt_daily_load (schedule on assets, asset events), report_refreshed_notice.

Airflow Variables (all optional; every asset feature is off until one is set; the DAG files read them when they are
parsed, so a change takes effect after the next parse of dags/dbt_daily_load.py, not at the next task):

``dbt_dataset_dependencies`` (JSON list of asset URIs)
    Non-empty: dbt_daily_load runs when EVERY listed asset has a new event, instead of on its clock schedule.
    Empty or unset: the clock schedule (DBT_DAILY_LOAD_SCHEDULE) applies, as before.
``dbt_emit_asset_events`` (true / false, default false)
    true: after a build dbt_daily_load announces every table it refreshed (one asset event per table, with the result
    in the event's `extra`).
"""

import logging
import os
import re
from datetime import datetime, timezone
from typing import Any, Callable

from utils.dbt_config import ASSET_URI_HOST_PREFIX, ASSET_URI_SCHEME, ENV
from utils.dbt_failure_report import ENVIRONMENT_NAMES, number_nl
from utils.dbt_runtime_overrides import parse_list

logger = logging.getLogger(__name__)

DEPENDENCIES_VARIABLE = "dbt_dataset_dependencies"
EMIT_VARIABLE = "dbt_emit_asset_events"
# The events of all tables of a build are emitted through this alias (the assets are made up at run time)
REFRESHED_ALIAS = "dbt_refreshed_tables"

_URI = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*://\S+$")
_NAME = re.compile(r"^[A-Za-z0-9_.$-]+$")


# ---------------------------------------------------------------------------
# URIs and Variables
# ---------------------------------------------------------------------------


def get_outlet_path(*args: str, environment: str = ENV) -> str:
    """The asset URI of a table or view: ``s2://workshop-<env>/<database>/<object>``.

    Two forms: ``get_outlet_path("dev_im_dimensional", "mart_olist_management_kpi")`` or the single string
    ``get_outlet_path("dev_im_dimensional/mart_olist_management_kpi")``. The one place the scheme is defined."""
    if len(args) == 1:
        parts = str(args[0]).split("/")
        if len(parts) != 2:
            raise ValueError(f"'{args[0]}' must be in 'database/object' format")
    elif len(args) == 2:
        parts = [str(args[0]), str(args[1])]
    else:
        raise ValueError("get_outlet_path takes (database, object) or one 'database/object' string")
    database, object_name = parts[0].strip(), parts[1].strip()
    if not database or not object_name:
        raise ValueError("database and object names cannot be empty")
    for part in (database, object_name):
        if not _NAME.match(part):
            raise ValueError(f"{part!r} is not a database or object name")
    host = f"{ASSET_URI_HOST_PREFIX}-{environment}"
    return f"{ASSET_URI_SCHEME}://{host}/{database}/{object_name}"


def parse_asset_dependencies(raw: Any) -> list[str]:
    """The URIs of ``dbt_dataset_dependencies``: a JSON list (or a comma / newline separated string). Empty -> [].
    Anything that is not ``scheme://...`` raises ValueError."""
    uris = parse_list(raw)
    for uri in uris:
        if not _URI.match(uri) or uri.lower().startswith("airflow:"):
            raise ValueError(f"{DEPENDENCIES_VARIABLE}: {uri!r} is not an asset URI (expected scheme://...)")
    return uris


def read_asset_dependencies(get_variable: Callable[..., Any]) -> list[str]:
    """``dbt_dataset_dependencies`` for the DAG's schedule. Called while the DAG file is parsed: a broken value or an
    unreachable Variable store must not remove the DAG, so it is logged and the clock schedule stays."""
    try:
        uris = parse_asset_dependencies(get_variable(DEPENDENCIES_VARIABLE, "[]"))
    except Exception as exc:
        logger.warning(f"{DEPENDENCIES_VARIABLE} ignored, the clock schedule applies: {exc}")
        return []
    if uris:
        logger.info(f"{DEPENDENCIES_VARIABLE}: scheduled on {uris}")
    return uris


def parse_bool(raw: Any, default: bool = False) -> bool:
    """true / false, also 1 / 0, yes / no, on / off (any case). Empty or unknown -> `default`."""
    if isinstance(raw, bool):
        return raw
    text = str(raw if raw is not None else "").strip().lower()
    if text in ("true", "1", "yes", "y", "on"):
        return True
    if text in ("false", "0", "no", "n", "off"):
        return False
    return default


def emit_events_enabled(get_variable: Callable[..., Any]) -> bool:
    """``dbt_emit_asset_events``; read at parse time too, so an unreachable Variable store means off."""
    try:
        return parse_bool(get_variable(EMIT_VARIABLE, "false"))
    except Exception as exc:
        logger.warning(f"{EMIT_VARIABLE} ignored, no asset events: {exc}")
        return False


def make_asset(uri: str):
    """The Airflow Asset of a URI. Every DAG builds its assets here, so a producer and a consumer agree on the identity."""
    from airflow.sdk import Asset

    return Asset(name=uri, uri=uri)


# ---------------------------------------------------------------------------
# Events after a build
# ---------------------------------------------------------------------------


def plan_events(payload: dict[str, Any] | None, image_tag: str, environment: str = ENV) -> list[tuple[str, dict[str, Any]]]:
    """[(asset URI, extra)] for the tables of `extract_refreshed_tables` (utils/dbt_state_utils.py): one event per
    refreshed table. `extra`: status, rows (the table's row count, see `add_row_counts`), rows_affected (what dbt
    reported, when it does), execution_time, invocation_id, dbt_image_tag, environment."""
    payload = payload or {}
    plan: list[tuple[str, dict[str, Any]]] = []
    for table in payload.get("tables") or []:
        uri = get_outlet_path(table["database"], table["table"], environment=environment)
        plan.append((uri, {
            "status": table.get("status", "success"),
            "rows": table.get("rows"),
            "rows_affected": table.get("rows_affected"),
            "execution_time": table.get("execution_time"),
            "invocation_id": payload.get("invocation_id"),
            "dbt_image_tag": image_tag,
            "environment": environment,
        }))
    return plan


def count_rows(tables: list[tuple[str, str]], env: dict[str, str] | None = None, connect: Callable[..., Any] | None = None) -> dict[tuple[str, str], int]:
    """{(database, table): row count} of the tables it could count. dbt Fusion's SingleStore driver reports no
    `rows_affected`, so the rows of a refreshed table are counted here, with the same SINGLESTORE_HOST / _USER / _PASS
    the dbt containers get. Never raises: without credentials, a driver or a table, that table is simply left out."""
    env = os.environ if env is None else env
    host, user, password = (env.get(key, "").strip() for key in ("SINGLESTORE_HOST", "SINGLESTORE_USER", "SINGLESTORE_PASS"))
    if not (tables and host and user and password):
        return {}
    counts: dict[tuple[str, str], int] = {}
    try:
        if connect is None:
            import MySQLdb  # the driver of the Airflow image (mysqlclient)

            connect = MySQLdb.connect
        conn = connect(host=host, user=user, passwd=password, port=int(env.get("SINGLESTORE_PORT", "3306")),
                       connect_timeout=10, read_timeout=60)
    except Exception as exc:
        logger.warning(f"Rows are not counted (no connection to {host}): {exc}")
        return {}
    try:
        cursor = conn.cursor()
        for database, table in tables:
            if not (_NAME.match(database) and _NAME.match(table)):
                continue
            try:
                cursor.execute(f"SELECT COUNT(*) FROM `{database}`.`{table}`")
                counts[(database, table)] = int(cursor.fetchone()[0])
            except Exception as exc:
                logger.warning(f"Rows of {database}.{table} are not counted: {exc}")
    finally:
        try:
            conn.close()
        except Exception:
            pass
    return counts


def add_row_counts(payload: dict[str, Any] | None, counter: Callable[[list[tuple[str, str]]], dict[tuple[str, str], int]] = count_rows) -> dict[str, Any]:
    """The payload of `extract_refreshed_tables` with a `rows` count on every table the counter could count."""
    payload = dict(payload or {})
    tables = [dict(t) for t in payload.get("tables") or []]
    counts = counter([(t["database"], t["table"]) for t in tables]) if tables else {}
    for table in tables:
        table["rows"] = counts.get((table["database"], table["table"]))
    payload["tables"] = tables
    return payload


# ---------------------------------------------------------------------------
# The notice (report_refreshed_notice)
# ---------------------------------------------------------------------------


def _field(event: Any, name: str) -> Any:
    return event.get(name) if isinstance(event, dict) else getattr(event, name, None)


def _aware(timestamp: Any) -> datetime:
    """The timestamp as an aware datetime (UTC when naive); anything else sorts first."""
    if not isinstance(timestamp, datetime):
        return datetime.min.replace(tzinfo=timezone.utc)
    return timestamp if timestamp.tzinfo else timestamp.replace(tzinfo=timezone.utc)


def collect_events(triggering: Any) -> list[dict[str, Any]]:
    """[{timestamp, extra}] from ``context['triggering_asset_events']`` (asset -> events), oldest first. Empty when
    the run was started by hand. An event that arrives through an alias is listed under the alias too (Airflow's
    TriggeringAssetEventsAccessor): it counts once, whatever number of keys list it."""
    if not triggering:
        return []
    groups = triggering.values() if hasattr(triggering, "values") else [triggering]
    events: list[dict[str, Any]] = []
    seen: set[Any] = set()
    for group in groups:
        for event in group or []:
            extra = _field(event, "extra")
            asset = _field(event, "asset")
            identity = (_field(event, "id"), str(_field(asset, "uri") if asset is not None else ""),
                        _field(event, "timestamp"), _field(event, "source_run_id"), _field(event, "source_task_id"))
            if identity in seen:
                continue
            seen.add(identity)
            events.append({"timestamp": _field(event, "timestamp"), "extra": extra if isinstance(extra, dict) else {}})
    return sorted(events, key=lambda e: _aware(e["timestamp"]))


MONTHS_NL = ("januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober",
             "november", "december")


def _when(timestamp: Any) -> str:
    """`21 november, 06:03 UTC` (Belgian date notation; UTC, as the events are stored)."""
    if not isinstance(timestamp, datetime):
        return "niet bekend"
    moment = _aware(timestamp).astimezone(timezone.utc)
    return f"{moment.day} {MONTHS_NL[moment.month - 1]}, {moment:%H:%M} UTC"


def render_refresh_notice(
    events: list[dict[str, Any]], environment: str, database: str, table: str
) -> tuple[str, str]:
    """(subject, plain-text body, in Flemish) saying that the management report was refreshed: when, by which version,
    how many rows. For non-technical readers, so the technical table name is not in the mail (`database` and `table`
    stay in the signature for the caller). `events` as `collect_events` returns them; with several only the newest is
    described. No events: the DAG was started by hand. The studio finds the mail by its subject."""
    where = ENVIRONMENT_NAMES.get(environment, environment)
    if not events:
        subject = f"[{where}] Testverwittiging: manueel gestart"
        body = (
            "Dit is een testverwittiging. Ze is manueel gestart en komt niet van een verversing van het "
            "managementrapport.\n"
            "De mail werkt. Een echte verwittiging zegt wanneer het rapport ververst is, met welke versie en met "
            "hoeveel rijen.\n\n"
            f"Omgeving: {where}\n"
        )
        return subject, body
    event = events[-1]
    extra = event.get("extra") or {}
    rows = extra.get("rows") if extra.get("rows") is not None else extra.get("rows_affected")
    rows_text = number_nl(rows) if isinstance(rows, (int, float)) and not isinstance(rows, bool) else "niet gemeld"
    version = extra.get("dbt_image_tag") or "niet bekend"
    lines = [
        "Het managementrapport is ververst.",
        "",
        f"Wanneer: {_when(event.get('timestamp'))}",
        f"Versie: {version}",
        f"Rijen in het rapport: {rows_text}",
        f"Omgeving: {where}",
    ]
    if len(events) > 1:
        lines.append(f"({len(events)} verversingen sinds de vorige verwittiging; dit is de nieuwste.)")
    lines.append("")
    return f"[{where}] Het managementrapport is ververst", "\n".join(lines)
