"""Plain words for the technical names of dbt checks, for every sentence the UI shows.

`source_not_null_raw_olist_raw_olist_orders_order_id` becomes "Orders feed: the order number is never empty". The
generic kinds (not_null, unique, accepted_values, relationships, freshness) are understood for sources and tables; a
name that cannot be taken apart still gets readable words (prefixes dropped, underscores turned into spaces).
Callers keep the technical name for the "technical details".

The same goes for database accounts (`svc_dev_dbt` -> "the development robot", `pu_dev1` -> "developer one") and
databases (`prd_edw_dv_bdv` -> "the working tables of production"): `plain_words` rewrites any sentence and
`plain_pair` moves the technical sentence to the details.
"""
from __future__ import annotations

import os
import re
from datetime import datetime, timezone

KINDS = ("unique_combination_of_columns", "expression_is_true", "not_null", "unique", "accepted_values", "relationships",
         "freshness")
PHRASES = {
    "not_null": "the {col} is never empty",
    "unique": "the {col} appears only once",
    "accepted_values": "the {col} only has allowed values",
    "relationships": "every {col} belongs to a known record",
    "freshness": "the data is recent enough",
    "expression_is_true": "the {col} has the expected format",
    "unique_combination_of_columns": "each combination of {col} appears only once",
}
CHECK_NAME = re.compile(r"^(?:dbt_utils_)?(source_)?(" + "|".join(KINDS) + r")_[a-z0-9_]+$")
# The hand-written checks of the project (`tests/*/assert_*.sql`) in the words of what they guard.
ASSERTS = {
    "assert_carrier_events_delivered": "Parcel scans feed: new scans arrived in the last 3 days",
    "assert_carrier_events_readable": "Parcel scans: every scan can be read",
    "assert_delivery_status_consistent": "Deliveries table: a delivered parcel always shows as delivered",
    "assert_web_checkouts_attributed": "Website conversion report: nearly every checkout matches an order",
}

# The six feeds and the main tables, in the words the audience hears.
TABLES = {
    "raw_olist_customers": "Customers feed",
    "raw_olist_orders": "Orders feed",
    "raw_olist_order_payments": "Payments feed",
    "raw_olist_order_reviews": "Reviews feed",
    "raw_carrier_tracking_events": "Parcel scans feed",
    "raw_web_events": "Website visits feed",
    "stg_olist_customers": "Customers (cleaned)",
    "stg_olist_orders": "Orders (cleaned)",
    "stg_carrier_events": "Parcel scans (cleaned)",
    "stg_web_events": "Website visits (cleaned)",
    "dim_olist_customers": "Customers table",
    "fct_olist_orders": "Orders table",
    "fct_olist_deliveries": "Deliveries table",
    "mart_olist_management_kpi": "Management report",
    "mart_web_conversion": "Website conversion report",
    "mart_web_journeys": "Website journeys report",
}
COLUMNS = {"order_id": "order number", "customer_id": "customer number", "customer_unique_id": "customer number",
           "review_id": "review number", "event_id": "scan number", "payment_type": "payment type",
           "order_status": "order status", "_ingested_at": "arrival time", "id": "number", "seq": "sequence number",
           "hkey_record": "record key", "pit_key": "history key", "customer_hkey": "customer key",
           "order_hkey": "order key", "last_loaded_date": "last load date"}
_LONGEST_FIRST = sorted(TABLES, key=len, reverse=True)
_DROPPED_PREFIXES = ("raw_olist_", "raw_carrier_", "raw_web_", "olist_", "raw_", "stg_", "dim_", "fct_", "mart_", "assert_")


def looks_like_check(name: object) -> bool:
    """True for the name of a generic dbt check (`not_null_...`, `source_unique_...`)."""
    return isinstance(name, str) and (bool(CHECK_NAME.match(name)) or name in ASSERTS)


def column_words(column: str) -> str:
    if column in COLUMNS:
        return COLUMNS[column]
    words = column.strip("_").replace("_", " ")
    return words[:-3] + " number" if words.endswith(" id") else words


def _everyday(text: str) -> str:
    for prefix in _DROPPED_PREFIXES:
        if text.startswith(prefix) and len(text) > len(prefix):
            text = text[len(prefix):]
            break
    return text.replace("_", " ").strip()


_HASH_TAIL = re.compile(r"_[0-9a-f]{10,}$")


def _known_tables() -> list[str]:
    return sorted({**{k: v for k, v in MODELS.items()}, **TABLES}, key=len, reverse=True)


def _table_title(key: str) -> str:
    return TABLES.get(key) or MODELS[key]


def _columns_words(columns: str, split: bool) -> str:
    """`order_id__payment_sequential` -> "order number and payment sequential"; one name for a plain column."""
    parts = [c for c in (columns.split("__") if split else [columns]) if c]
    words = [column_words(c) for c in parts]
    return " and ".join(words) if len(words) < 3 else ", ".join(words[:-1]) + " and " + words[-1]


def check_title(name: str) -> str:
    """The plain sentence for a check name; a name that is not a check is only made readable. Every dbt check of the
    project has a title; a check on an unknown table is "Check on <table>"."""
    if name in ASSERTS:
        return ASSERTS[name]
    found = CHECK_NAME.match(name)
    if not found:
        return _everyday(name).capitalize() or name
    kind = found.group(2)
    prefix = name[:found.start(2)]
    rest = name[len(prefix) + len(kind) + 1:]
    if len(name) > 60:
        rest = _HASH_TAIL.sub("", rest)   # dbt shortens very long names with a hash at the end
    head = rest.split("__", 1)[0] if kind != "unique_combination_of_columns" else rest
    for table in _known_tables():
        at = ("_" + head).find("_" + table + "_")
        title = _table_title(table)
        if kind == "freshness" and head.endswith(table) and at < 0:
            return f"{title}: {PHRASES[kind]}"
        if at >= 0 and head[at + len(table) + 1:]:
            column = head[at + len(table) + 1:]
            if kind == "unique_combination_of_columns":
                multi = "__" in rest
                column = rest.split(table + "_", 1)[1] if multi else column
                return f"{title}: {PHRASES[kind].format(col=_columns_words(column, multi))}"
            words = "visit number" if column == "event_id" and "web" in table else column_words(column)
            return f"{title}: {PHRASES[kind].format(col=words)}"
    words = _everyday(head.split("__", 1)[0])
    return f"Check on {words}" if words else name


# --- accounts and databases ------------------------------------------------------------------------------------

ENV_WORDS = {"dev": "development", "val": "validation", "prd": "production"}
NUMBER_WORDS = {"1": "one", "2": "two", "3": "three"}
ACCOUNT = re.compile(r"\b(svc_(?:dev|val|prd|ci)_dbt|pu_dev[123]|pu_admin)\b")
DATABASE_PARTS = {"landing": "the incoming data of {env}", "edw_dv_rdv": "the working tables of {env}",
                  "edw_dv_bdv": "the working tables of {env}", "im_dimensional": "the report tables of {env}",
                  "vdp_mgmt": "the logbook of {env}"}
DATABASE = re.compile(r"\b(?:(dev|val|prd)_(" + "|".join(DATABASE_PARTS) + r")|(vdp_ci))\b(?:\.`?([a-z][a-z0-9_]*)`?)?")
_ANY_NAME = re.compile(ACCOUNT.pattern + "|" + DATABASE.pattern)


def account_words(name: str) -> str | None:
    """`svc_dev_dbt` -> "the development robot"; None for a name that is not a known account."""
    if not ACCOUNT.fullmatch(name):
        return None
    if name == "svc_ci_dbt":
        return "the checking robot"
    if name == "pu_admin":
        return "the administrator"
    if name.startswith("pu_dev"):
        return f"developer {NUMBER_WORDS[name[-1]]}"
    return f"the {ENV_WORDS[name.split('_')[1]]} robot"


def tables_in(database: str) -> str:
    """"tables in production" for every database of an environment; other databases keep their plain name."""
    found = DATABASE.fullmatch(database)
    if found and found.group(1):
        return f"tables in {ENV_WORDS[found.group(1)]}"
    return database_words(database) or f"tables in {database}"


def database_words(name: str) -> str | None:
    """`prd_edw_dv_bdv` -> "the working tables of production"; None for a name that is not a known database."""
    found = DATABASE.fullmatch(name)
    if not found:
        return None
    return "the sandbox for checks" if found.group(3) else DATABASE_PARTS[found.group(2)].format(env=ENV_WORDS[found.group(1)])


def _table_words(table: str) -> str:
    return "the " + (TABLES.get(table) or _everyday(table)).lower()


# Phrases of scenario files and actions that say too little, in the words they get on screen.
REWORDINGS = (
    (re.compile(r"\bReading the numbers\b"), "Reading which version each environment runs"),
)


def _reworded(text: str) -> str:
    for pattern, words in REWORDINGS:
        text = pattern.sub(words, text)
    return text


def plain_words(text: str) -> str:
    """The text with every account and database name replaced by plain words (a `db.table` becomes "the orders
    feed in the incoming data of development"). Text without such a name is returned unchanged."""
    def swap(found: re.Match) -> str:
        whole = found.group(0)
        if ACCOUNT.fullmatch(whole):
            words = account_words(whole)
        else:
            db = DATABASE.fullmatch(whole)
            words = database_words(whole.split(".")[0])
            if db.group(4):
                words = f"{_table_words(db.group(4))} in {words}"
        before = found.string[max(0, found.start() - 2):found.start()]
        if found.start() == 0 or before in (". ", "\n"):
            words = words[0].upper() + words[1:]
        return words

    return _reworded(_ANY_NAME.sub(swap, text))


def technical_names(text: str) -> list[str]:
    """The account and database names in `text`, in order, without repeats."""
    seen: list[str] = []
    for found in _ANY_NAME.finditer(text):
        name = found.group(0).split(".")[0]
        if name not in seen:
            seen.append(name)
    return seen


def plain_pair(summary: str, details: str = "") -> tuple[str, str]:
    """A sentence for the audience and its details: when the sentence held technical names, the original sentence
    is put in front of the details."""
    words = plain_words(summary)
    if words == summary:
        return summary, details
    return words, summary + ("\n" + details if details else "")


# --- resource types and dbt model names ---------------------------------------------------------------------------

RESOURCE_WORDS = {"model": "table", "test": "check", "unit_test": "check", "source": "feed"}
# The project's main models in the words the audience hears (the feeds themselves are in TABLES).
MODELS = {
    "stg_olist_customers": "Cleaned customers table",
    "stg_olist_orders": "Cleaned orders table",
    "stg_olist_order_payments": "Cleaned payments table",
    "stg_olist_order_reviews": "Cleaned reviews table",
    "stg_carrier_events": "Cleaned parcel scans table",
    "stg_web_events": "Cleaned website visits table",
    "dim_olist_customers": "Customers table",
    "fct_olist_orders": "Orders table",
    "fct_olist_deliveries": "Deliveries table",
    "fct_olist_order_payments": "Payments table",
    "fct_olist_order_reviews": "Reviews table",
    "int_web_top_pages": "Most visited pages table",
    "mart_olist_management_kpi": "Management report table",
    "mart_web_conversion": "Website conversion table",
    "mart_web_journeys": "Website journeys table",
    "ref_dummy_numbers": "Practice numbers table",
    "dummy_loading_history": "Practice loading history table",
    "dummy_reload_date_window": "Practice rebuild table",
    "dummy_reload_hkey_mod": "Practice rebuild table (by key)",
    "dummy_reload_pit_with_deps": "Practice rebuild table (with dependencies)",
    "datum": "Calendar table",
    "datum_incr": "Calendar table (incremental)",
    # the platform's own tables (logbooks and overviews of the database), built like every other table
    "dbt_active_test_failures": "Failing data checks",
    "dbt_foreign_create_user": "Accounts made outside the code",
    "dbt_model_info": "Table details",
    "dbt_models": "List of tables",
    "dbt_orphaned_objects": "Leftover tables",
    "dbt_run_results": "Run logbook",
    "dbt_sources": "List of data feeds",
    "dbt_table_grants": "Access rights",
    "dbt_table_performance": "Table speed",
    "singlestore_column_info": "Column details",
    "singlestore_table_info": "Database table details",
}
_MODEL_PREFIXES = ("stg_", "int_", "dim_", "fct_", "ref_", "mart_")
_MODEL_DROP = ("stg_", "int_", "dim_", "fct_", "ref_", "mart_", "olist_")
_BARE_NAME = re.compile(r"^[a-z][a-z0-9_]*$")


def model_title(name: str) -> str | None:
    """The plain name of a dbt model or feed, or None when `name` does not look like one. Known models come from a
    small table; any other name with a model prefix (`stg_`, `int_`, `dim_`, `fct_`, `ref_`) is made readable
    ("int_web_top_pages" -> "Web top pages table"). A schema prefix (`demo_orders_calendar_fct_olist_orders`) is ignored."""
    if not isinstance(name, str) or not _BARE_NAME.match(name) or looks_like_check(name):
        return None
    if name in TABLES and name.startswith("raw_"):
        return TABLES[name]
    if name in MODELS:
        return MODELS[name]
    for known in sorted(MODELS, key=len, reverse=True):
        if name.endswith("_" + known):
            return MODELS[known]
    if name.startswith(_MODEL_PREFIXES):
        words = name
        for prefix in _MODEL_DROP:
            if words.startswith(prefix):
                words = words[len(prefix):]
        words = words.replace("_", " ").strip()
        return (words[:1].upper() + words[1:] + " table") if words else None
    return None


MASK = "'••••••'"
_LITERAL = r"""'[^']*'|"[^"]*"|\{\{.*?\}\}|[^\s,;)]+"""
_SECRET_RULES = (
    re.compile(r"(?i)(\bIDENTIFIED\s+BY\s+(?:PASSWORD\s+)?)(" + _LITERAL + ")"),
    re.compile(r"(?i)(\b\w*(?:password|passwd|pwd|secret)\w*[\"']?\s*[:=]\s*)(" + _LITERAL + ")"),
    re.compile(r"(?i)(\bPASSWORD\s+)('[^']*'|\"[^\"]*\")"),
)


def mask_secrets(line: str) -> str:
    """The line with the literal after `IDENTIFIED BY`, `PASSWORD`, `password:` / `password=` (and similar) replaced
    by '••••••'; the rest of the line is kept."""
    for rule in _SECRET_RULES:
        line = rule.sub(lambda m: m.group(1) + (m.group(2) if m.group(2) == MASK else MASK), line)
    return line


def plain_cells(cells: dict) -> tuple[dict, list[str]]:
    """The cells with every check name replaced by plain words, plus the technical names that were replaced."""
    out, technical = {}, []
    for key, value in cells.items():
        if isinstance(value, str) and value in RESOURCE_WORDS:
            value = RESOURCE_WORDS[value]
        elif looks_like_check(value):
            technical.append(value)
            value = check_title(value)
        elif (title := model_title(value)) is not None:
            technical.append(value)
            value = title
        elif isinstance(value, str) and (names := technical_names(value)):
            technical.extend(n for n in names if n not in technical)
            value = plain_words(value)
        out[key] = value
    return out, technical


# where the audience lives: STUDIO_TIME_ZONE (compose passes the host's TZ), else the dbt project's own time zone
LOCAL_ZONE = os.environ.get("STUDIO_TIME_ZONE") or "Europe/Brussels"


def local_time_words(stamp: str | None) -> str | None:
    """An ISO time from GitLab or dbt in local time, as people say it: "15 November, 18:06" (no "UTC"). None when the
    text is no time. Without time-zone data on the machine the time stays in UTC."""
    if not stamp:
        return None
    try:
        when = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    except ValueError:
        return None
    if when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    try:
        from zoneinfo import ZoneInfo
        when = when.astimezone(ZoneInfo(LOCAL_ZONE))
    except Exception:  # noqa: BLE001 - no tz database: keep UTC rather than fail a sentence
        when = when.astimezone(timezone.utc)
    return f"{when.day} {when.strftime('%B')}, {when:%H:%M}"
