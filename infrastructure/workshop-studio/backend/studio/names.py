"""Plain words (spoken Flemish) for the technical names of dbt checks, for every sentence the UI shows.

`source_not_null_raw_olist_raw_olist_orders_order_id` becomes "Bestellingenfeed: het bestelnummer is nooit leeg". The
generic kinds (not_null, unique, accepted_values, relationships, freshness) are understood for sources and tables; a
name that cannot be taken apart still gets readable words (prefixes dropped, underscores turned into spaces).
Callers keep the technical name for the "technical details".

The same goes for database accounts (`svc_dev_dbt` -> "de robot van development", `pu_dev1` -> "developer één") and
databases (`prd_edw_dv_bdv` -> "de werktabellen van productie"): `plain_words` rewrites any sentence and
`plain_pair` moves the technical sentence to the details.

Every noun phrase that names a database, a table or a feed carries its own article ("de werktabellen van
productie"), so a template can put it in a sentence without guessing de or het (`with_article` does it for a bare
title).
"""
from __future__ import annotations

import os
import re
from datetime import date, datetime, timezone

KINDS = ("unique_combination_of_columns", "expression_is_true", "not_null", "unique", "accepted_values", "relationships",
         "freshness")
# `{col}` is the column with its article ("het bestelnummer"), `{each}` with "elk/elke", `{bare}` without article.
PHRASES = {
    "not_null": "{col} is nooit leeg",
    "unique": "{col} komt maar één keer voor",
    "accepted_values": "{col} heeft alleen toegelaten waarden",
    "relationships": "{each} hoort bij een bestaande rij",
    "freshness": "de data is recent genoeg",
    "expression_is_true": "{col} heeft het verwachte formaat",
    "unique_combination_of_columns": "elke combinatie van {bare} komt maar één keer voor",
}
CHECK_NAME = re.compile(r"^(?:dbt_utils_)?(source_)?(" + "|".join(KINDS) + r")_[a-z0-9_]+$")
# The hand-written checks of the project (`tests/*/assert_*.sql`) in the words of what they guard.
ASSERTS = {
    "assert_carrier_events_delivered": "Pakketscanfeed: de laatste 3 dagen zijn er nieuwe scans binnengekomen",
    "assert_carrier_events_readable": "Pakketscans: elke scan is leesbaar",
    "assert_delivery_status_consistent": "Leveringstabel: een geleverd pakket staat altijd als geleverd",
    "assert_web_checkouts_attributed": "Websiteconversierapport: bijna elke checkout hoort bij een bestelling",
}

# The six feeds and the main tables, in the words the audience hears.
TABLES = {
    "raw_olist_customers": "Klantenfeed",
    "raw_olist_orders": "Bestellingenfeed",
    "raw_olist_order_payments": "Betalingenfeed",
    "raw_olist_order_reviews": "Reviewfeed",
    "raw_carrier_tracking_events": "Pakketscanfeed",
    "raw_web_events": "Websitebezoekfeed",
    "raw_olist_returns": "Retourenfeed",
    "stg_olist_customers": "Klanten (opgekuist)",
    "stg_olist_orders": "Bestellingen (opgekuist)",
    "stg_carrier_events": "Pakketscans (opgekuist)",
    "stg_web_events": "Websitebezoeken (opgekuist)",
    "dim_olist_customers": "Klantentabel",
    "fct_olist_orders": "Bestellingentabel",
    "fct_olist_deliveries": "Leveringstabel",
    "mart_olist_management_kpi": "Managementrapport",
    "mart_web_conversion": "Websiteconversierapport",
    "mart_web_journeys": "Websitetrajectenrapport",
}
COLUMNS = {"order_id": "bestelnummer", "customer_id": "klantnummer", "customer_unique_id": "klantnummer",
           "review_id": "reviewnummer", "event_id": "scannummer", "payment_type": "betaalmethode",
           "order_status": "bestelstatus", "_ingested_at": "aankomsttijd", "id": "nummer", "seq": "volgnummer",
           "hkey_record": "recordsleutel", "pit_key": "historieksleutel", "customer_hkey": "klantsleutel",
           "order_hkey": "bestelsleutel", "last_loaded_date": "laatste laaddatum",
           "returned_at": "retourtijd", "return_channel": "retourkanaal", "payment_sequential": "betalingsvolgnummer"}
# the columns that take "het" (every other column takes "de"); a word ending in "nummer" takes "het" too
HET_WORDS = {"retourkanaal"}
_LONGEST_FIRST = sorted(TABLES, key=len, reverse=True)
_DROPPED_PREFIXES = ("raw_olist_", "raw_carrier_", "raw_web_", "olist_", "raw_", "stg_", "dim_", "fct_", "mart_", "assert_")


def looks_like_check(name: object) -> bool:
    """True for the name of a generic dbt check (`not_null_...`, `source_unique_...`)."""
    return isinstance(name, str) and (bool(CHECK_NAME.match(name)) or name in ASSERTS)


MONTHS = ("januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober",
          "november", "december")
SHORT_MONTHS = ("jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec")
WEEKDAYS = ("maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag", "zondag")   # Monday first


def format_number(value: int | float) -> str:
    """A number the Belgian way: 3.432 (thousands with a dot), 92,8 (decimals with a comma)."""
    if isinstance(value, float) and value != int(value):
        return f"{value:g}".replace(".", ",")
    return f"{int(value):,}".replace(",", ".")


def day_words(day: object) -> str:
    """An ISO day (`2026-11-15`, also a `date`) as people say it: "15 november"; any other text is returned as it is."""
    try:
        when = day if isinstance(day, date) else date.fromisoformat(str(day)[:10])
    except ValueError:
        return str(day)
    return f"{when.day} {MONTHS[when.month - 1]}"


def column_words(column: str) -> str:
    if column in COLUMNS:
        return COLUMNS[column]
    words = column.strip("_").replace("_", " ")
    return words[:-3] + " nummer" if words.endswith(" id") else words


def _column_article(words: str) -> str:
    return "het" if words in HET_WORDS or words.endswith("nummer") else "de"


def column_with_article(column: str) -> str:
    """`order_id` -> "het bestelnummer", `order_hkey` -> "de bestelsleutel"."""
    words = column_words(column)
    return f"{_column_article(words)} {words}"


def with_article(title: str) -> str:
    """A plain title with its article and in lower case: "Bestellingenfeed" -> "de bestellingenfeed",
    "Managementrapport" -> "het managementrapport". A plural title ("Klanten (opgekuist)") takes "de"."""
    words = title[:1].lower() + title[1:]
    head = words.split(" (")[0]
    return ("het " if head.endswith(("rapport", "logboek")) else "de ") + words


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
    """`order_id__payment_sequential` -> "bestelnummer en betalingsvolgnummer"; one name for a plain column."""
    parts = [c for c in (columns.split("__") if split else [columns]) if c]
    words = [column_words(c) for c in parts]
    return " en ".join(words) if len(words) < 3 else ", ".join(words[:-1]) + " en " + words[-1]


def _phrase(kind: str, column: str, split: bool = False) -> str:
    """The sentence part of a check kind for a column (or, for `unique_combination_of_columns`, several)."""
    words = column_words(column) if not split else ""
    article = _column_article(words)
    return PHRASES[kind].format(col=f"{article} {words}", each=f"{'elk' if article == 'het' else 'elke'} {words}",
                                bare=_columns_words(column, split))


def check_title(name: str) -> str:
    """The plain sentence for a check name; a name that is not a check is only made readable. Every dbt check of the
    project has a title; a check on an unknown table is "Check op <table>"."""
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
            return f"{title}: {PHRASES[kind].format(col='', each='', bare='')}"
        if at >= 0 and head[at + len(table) + 1:]:
            column = head[at + len(table) + 1:]
            if kind == "unique_combination_of_columns":
                multi = "__" in rest
                column = rest.split(table + "_", 1)[1] if multi else column
                return f"{title}: {_phrase(kind, column, multi)}"
            if column == "event_id" and "web" in table:
                return f"{title}: {PHRASES[kind].format(col='het bezoeknummer', each='elk bezoeknummer', bare='bezoeknummer')}"
            return f"{title}: {_phrase(kind, column)}"
    words = _everyday(head.split("__", 1)[0])
    return f"Check op {words}" if words else name


# --- accounts and databases ------------------------------------------------------------------------------------

ENV_WORDS = {"dev": "development", "val": "validatie", "prd": "productie"}
NUMBER_WORDS = {"1": "één", "2": "twee", "3": "drie"}
ACCOUNT = re.compile(r"\b(svc_(?:dev|val|prd|ci)_dbt|pu_dev[123]|pu_admin)\b")
DATABASE_PARTS = {"landing": "de binnenkomende data van {env}", "edw_dv_rdv": "de werktabellen van {env}",
                  "edw_dv_bdv": "de werktabellen van {env}", "im_dimensional": "de rapporttabellen van {env}",
                  "vdp_mgmt": "het logboek van {env}"}
DATABASE = re.compile(r"\b(?:(dev|val|prd)_(" + "|".join(DATABASE_PARTS) + r")|(vdp_ci))\b(?:\.`?([a-z][a-z0-9_]*)`?)?")
_ANY_NAME = re.compile(ACCOUNT.pattern + "|" + DATABASE.pattern)


def account_words(name: str) -> str | None:
    """`svc_dev_dbt` -> "de robot van development"; None for a name that is not a known account."""
    if not ACCOUNT.fullmatch(name):
        return None
    if name == "svc_ci_dbt":
        return "de robot van de testrun"
    if name == "pu_admin":
        return "de beheerder"
    if name.startswith("pu_dev"):
        return f"developer {NUMBER_WORDS[name[-1]]}"
    return f"de robot van {ENV_WORDS[name.split('_')[1]]}"


def tables_in(database: str) -> str:
    """"de tabellen in productie" for every database of an environment; other databases keep their plain name."""
    found = DATABASE.fullmatch(database)
    if found and found.group(1):
        return f"de tabellen in {ENV_WORDS[found.group(1)]}"
    return database_words(database) or f"de tabellen in {database}"


def database_words(name: str) -> str | None:
    """`prd_edw_dv_bdv` -> "de werktabellen van productie"; None for a name that is not a known database."""
    found = DATABASE.fullmatch(name)
    if not found:
        return None
    return "de sandbox voor de testrun" if found.group(3) else DATABASE_PARTS[found.group(2)].format(env=ENV_WORDS[found.group(1)])


def _table_words(table: str) -> str:
    return with_article(TABLES.get(table) or _everyday(table))


# Phrases of scenario files and actions that say too little, in the words they get on screen.
REWORDINGS = (
    (re.compile(r"\b(?:De nummers aflezen|Reading the numbers)\b"), "Nakijken welke versie elke omgeving draait"),
)


def _reworded(text: str) -> str:
    for pattern, words in REWORDINGS:
        text = pattern.sub(words, text)
    return text


def plain_words(text: str) -> str:
    """The text with every account and database name replaced by plain words (a `db.table` becomes "de bestellingenfeed
    in de binnenkomende data van development"). Text without such a name is returned unchanged."""
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

RESOURCE_WORDS = {"model": "tabel", "test": "check", "unit_test": "check", "source": "feed"}
# The project's main models in the words the audience hears (the feeds themselves are in TABLES).
MODELS = {
    "stg_olist_customers": "Opgekuiste klantentabel",
    "stg_olist_orders": "Opgekuiste bestellingentabel",
    "stg_olist_order_payments": "Opgekuiste betalingstabel",
    "stg_olist_order_reviews": "Opgekuiste reviewtabel",
    "stg_carrier_events": "Opgekuiste pakketscantabel",
    "stg_web_events": "Opgekuiste websitebezoektabel",
    "dim_olist_customers": "Klantentabel",
    "fct_olist_orders": "Bestellingentabel",
    "fct_olist_deliveries": "Leveringstabel",
    "fct_olist_order_payments": "Betalingstabel",
    "fct_olist_order_reviews": "Reviewtabel",
    "int_web_top_pages": "Tabel meest bezochte pagina's",
    "mart_olist_management_kpi": "Managementrapporttabel",
    "mart_olist_customer_cohorts": "Klantencohortentabel",
    "mart_web_conversion": "Websiteconversietabel",
    "mart_web_journeys": "Websitetrajectentabel",
    "ref_dummy_numbers": "Oefentabel met getallen",
    "dummy_loading_history": "Oefentabel laadhistoriek",
    "dummy_reload_date_window": "Oefentabel voor herbouw",
    "dummy_reload_hkey_mod": "Oefentabel voor herbouw (per sleutel)",
    "dummy_reload_pit_with_deps": "Oefentabel voor herbouw (met afhankelijkheden)",
    "datum": "Kalendertabel",
    "datum_incr": "Kalendertabel (incrementeel)",
    # the platform's own tables (logbooks and overviews of the database), built like every other table
    "dbt_active_test_failures": "Mislukte datachecks",
    "dbt_foreign_create_user": "Accounts die buiten de code zijn aangemaakt",
    "dbt_model_info": "Tabeldetails",
    "dbt_models": "Lijst van tabellen",
    "dbt_orphaned_objects": "Overgebleven tabellen",
    "dbt_run_results": "Runlogboek",
    "dbt_sources": "Lijst van feeds",
    "dbt_table_grants": "Toegangsrechten",
    "dbt_table_performance": "Snelheid van de tabellen",
    "singlestore_column_info": "Kolomdetails",
    "singlestore_table_info": "Tabeldetails van de database",
}
_MODEL_PREFIXES = ("stg_", "int_", "dim_", "fct_", "ref_", "mart_")
_MODEL_DROP = ("stg_", "int_", "dim_", "fct_", "ref_", "mart_", "olist_")
_BARE_NAME = re.compile(r"^[a-z][a-z0-9_]*$")


def model_title(name: str) -> str | None:
    """The plain name of a dbt model or feed, or None when `name` does not look like one. Known models come from a
    small table; any other name with a model prefix (`stg_`, `int_`, `dim_`, `fct_`, `ref_`) is made readable
    ("int_web_something" -> "Tabel web something"). A schema prefix (`demo_orders_calendar_fct_olist_orders`) is ignored."""
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
        return ("Tabel " + words) if words else None
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
    """An ISO time from GitLab or dbt in local time, as people say it: "15 november, 18:06" (no "UTC"). None when the
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
    return f"{when.day} {MONTHS[when.month - 1]}, {when:%H:%M}"
