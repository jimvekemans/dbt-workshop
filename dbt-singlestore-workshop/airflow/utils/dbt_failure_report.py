"""One failure e-mail per DAG run: parse dbt text output (and run_results.json), render HTML, send it over SMTP.

Pure Python: no Airflow imports, so it is unit-testable on a host without Airflow (tests/test_failure_report.py).
The Airflow side (collecting the logs of the failed tasks, the `send_failure_report` task) lives in dbt_dag_utils.py.

The patterns target the text output of dbt 2 ("Fusion", 2.0.11) as captured from this stack's Airflow task logs:

    Succeeded model dim_olist_customers (table) [30 of 49 in 0.64s]
       Failed test  source_not_null_raw_olist_raw_olist_orders_order_id (models/edw_dv_bdv/sources_olist.yml:40:17)
                    [2 of 49 in 0.00s]   (one line in the log)
      Skipped model mart_olist_management_kpi (table) [46 of 49]
    Test failed (6 failed row(s)): source_not_null_raw_olist_raw_olist_orders_order_id
    Summary: 50 total | 43 success | 1 error | 6 skipped

NOT captured yet: a FAILED MODEL line. It is assumed to have the shape of the test line
(`Failed model <name> (<materialization>) [n of m in 0.1s]`); the regex handles it, but verify it on the first real
model failure. Parse-time failures print `[error] [Class (dbt1048)]: message` followed by `  --> path:line:col`.
"""

import html
import json
import logging
import os
import re
import smtplib
from dataclasses import dataclass, field
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from typing import Any

logger = logging.getLogger(__name__)

TAIL_LINES = 40
MAX_BLOCK_LINES = 15
MAX_MESSAGE_CHARS = 700
MAX_SKIPPED_LISTED = 30
RESOURCE_TYPES = ("model", "test", "seed", "snapshot", "source", "operation", "hook", "unit_test")
TABLE_TYPES = ("model", "seed", "snapshot")

PATTERNS: dict[str, re.Pattern] = {
    "ansi": re.compile(r"\x1b\[[0-9;?]*[A-Za-z]"),
    # Airflow text log prefix: [2026-01-01T10:00:00.123+0000] {file.py:12} INFO - message
    "airflow_prefix": re.compile(r"^\[\d{4}-\d\d-\d\d[T ][^\]]*\]\s*(?:\{[^}]*\}\s*)?(?:[A-Z]+\s+-\s+)?"),
    # optional `10:00:01  ` timestamp in front of a line
    "dbt_prefix": re.compile(r"^\d\d:\d\d:\d\d(?:\.\d+)?(?:\s+\[\w+\s*\]:)?\s+"),
    "logger_suffix": re.compile(r"\s+\[(?:logger|chan)=[^\]]*\]\s*$"),
    # dbt 2 result line: `<Status> <type> <name> [(<materialization or file:line:col>)] [n of m [in 0.64s]]`
    "result": re.compile(
        r"^(?P<status>Succeeded|Failed|Warned|Skipped|Reused|Errored|Passed)\s+(?P<rtype>\w+)\s+(?P<name>\S+)"
        r"(?:\s+\((?P<extra>[^)]*)\))?(?:\s+\[(?P<n>\d+) of (?P<m>\d+)(?:\s+in\s+(?P<dur>[\d.]+\s*m?s))?\])?\s*$"),
    # `Test failed (6 failed row(s)): <test name>`, printed after the run under "Test Failures"
    "test_detail": re.compile(r"^Test failed \((?P<rows>\d+) failed row\(s\)\):\s*(?P<name>\S+)\s*$"),
    # `[error] [DependencyNotFound (dbt1048)]: msg`; also accepted: `[error]: msg`, `error: dbt1005: msg`
    "error_head": re.compile(r"^(?:\[error\]\s*:?\s*|error(?:\[[^\]]*\])?:\s*)(?P<msg>.*)$", re.IGNORECASE),
    # `Error materializing test test.dbt_swf.<name>.<hash> (path)`: the failed node inside an error message
    "materializing": re.compile(r"Error materializing (?P<rtype>\w+) (?P=rtype)\.[^.\s]+\.(?P<name>[^.\s]+)"),
    "location": re.compile(r"^-->\s*(?P<path>[^\s:]+)(?P<pos>(?::\d+)*)\s*$"),
    "finished": re.compile(r"^Finished\s+'(?P<cmd>[^']+)'\s+(?:successfully|with\s+(?P<rest>.+?))"
                           r"(?:\s+for target\s+'[^']*')?(?:\s*\[[^\]]*\])?\s*$"),
    "summary": re.compile(r"^Summary:\s*(?P<kv>.+)$"),
    "processed": re.compile(r"^Processed:\s*(?P<kv>.+)$"),
    "banner": re.compile(r"^={3,}"),
    "rows_in_text": re.compile(r"(\d+)\s+(?:failed\s+)?(?:rows?|failures?)|Got\s+(\d+)\s+results?"),
}


@dataclass
class Finding:
    node: str
    resource_type: str = ""
    message: str = ""
    duration: str = ""
    failures: int | None = None
    unique_id: str = ""
    location: str = ""
    materialization: str = ""


@dataclass
class Report:
    errors: list[Finding] = field(default_factory=list)
    warnings: list[Finding] = field(default_factory=list)
    skipped: list[Finding] = field(default_factory=list)
    counts: dict[str, int] = field(default_factory=lambda: {"pass": 0, "warn": 0, "error": 0, "skip": 0})
    summary: str = ""
    processed: str = ""
    hooks: int = 0                      # hooks dbt reports under `Processed:`; dbt counts them as nodes, the mail does not
    dbt_success: int | None = None      # `N success` of dbt's own `Summary:` line (includes the hooks)
    tail: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Parsing
# ---------------------------------------------------------------------------


def plain_line(raw: str) -> str:
    """One log line without ANSI codes; an Airflow 3 JSON log line is replaced by its event text."""
    line = PATTERNS["ansi"].sub("", raw).rstrip("\r\n")
    stripped = line.strip()
    if stripped.startswith("{") and stripped.endswith("}"):
        try:
            data = json.loads(stripped)
            if isinstance(data, dict) and "event" in data:
                return str(data["event"])
        except ValueError:
            pass
    return line


def normalize_line(raw: str) -> str:
    """One log line without ANSI codes and without Airflow / dbt prefixes (leading spaces are dropped too)."""
    line = PATTERNS["airflow_prefix"].sub("", plain_line(raw).lstrip())
    line = PATTERNS["logger_suffix"].sub("", line)
    return PATTERNS["dbt_prefix"].sub("", line).rstrip()


def _short_name(name: str) -> str:
    """`db.schema.name` / `model.pkg.name` -> `name`."""
    return name.strip("`'\"").split(".")[-1]


def _is_terminator(line: str) -> bool:
    return any(PATTERNS[k].match(line) for k in ("result", "test_detail", "error_head", "finished", "summary",
                                                 "processed", "banner"))


def _rows_in(text: str) -> int | None:
    match = PATTERNS["rows_in_text"].search(text or "")
    return int(next(g for g in match.groups() if g)) if match else None


def _key_values(text: str) -> dict[str, int]:
    """`50 total | 43 success | 1 error | 6 skipped` -> {"total": 50, "success": 43, ...}"""
    return {m.group(2).lower(): int(m.group(1)) for m in re.finditer(r"(\d+)\s+([A-Za-z_]+)", text)}


def parse_log(text: str) -> Report:
    """Structured findings from dbt 2 text output, tolerant of Airflow prefixes, JSON log lines and ANSI codes."""
    report = Report()
    lines = [normalize_line(raw) for raw in text.splitlines()]
    report.tail = [plain_line(raw).rstrip() for raw in text.splitlines()[-TAIL_LINES:]]
    results: dict[str, Finding] = {}       # failed result lines by node name (insertion order = log order)
    warned: dict[str, Finding] = {}
    details: dict[str, int] = {}           # `Test failed (N failed row(s))` by test name
    blocks: list[Finding] = []             # `[error]` blocks; node may be empty (project-level)
    seen = {"pass": 0, "warn": 0, "error": 0, "skip": 0}
    summary_counts: dict[str, int] = {}
    finished_errors = finished_warnings = None

    i = 0
    while i < len(lines):
        line = lines[i].strip()
        i += 1
        if not line:
            continue

        match = PATTERNS["result"].match(line)
        if match:
            status, rtype, name = match.group("status"), match.group("rtype"), _short_name(match.group("name"))
            extra = match.group("extra") or ""
            finding = Finding(name, rtype, duration=match.group("dur") or "",
                              location=extra if rtype == "test" else "",
                              materialization="" if rtype == "test" else extra)
            if status in ("Succeeded", "Passed", "Reused"):
                seen["pass"] += 1
            elif status == "Skipped":
                seen["skip"] += 1
                report.skipped.append(finding)
            elif status == "Warned":
                seen["warn"] += 1
                warned[name] = finding
            else:
                seen["error"] += 1
                results[name] = finding
            continue

        match = PATTERNS["test_detail"].match(line)
        if match:
            details[match.group("name")] = int(match.group("rows"))
            continue
        match = PATTERNS["finished"].match(line)
        if match:
            rest = match.group("rest") or ""
            errs, warns = re.search(r"(\d+)\s+errors?", rest), re.search(r"(\d+)\s+warnings?", rest)
            finished_errors = int(errs.group(1)) if errs else 0
            finished_warnings = int(warns.group(1)) if warns else 0
            continue
        match = PATTERNS["summary"].match(line)
        if match:
            report.summary = line
            summary_counts = _key_values(match.group("kv"))
            continue
        match = PATTERNS["processed"].match(line)
        if match:
            report.processed = line
            report.hooks = _key_values(match.group("kv")).get("hook", _key_values(match.group("kv")).get("hooks", 0))
            continue

        head = PATTERNS["error_head"].match(line)
        if not head:
            continue
        body = [head.group("msg").strip()]
        node = rtype = location = ""
        blank_run = 0
        while i < len(lines) and len(body) < MAX_BLOCK_LINES:
            nxt = lines[i].strip()
            if not nxt:
                blank_run += 1
                if blank_run >= 2:
                    break
                i += 1
                continue
            if _is_terminator(nxt):
                break
            blank_run = 0
            pointer = PATTERNS["location"].match(nxt)
            if pointer and not location:
                location = pointer.group("path") + pointer.group("pos")
                if not node:
                    node, rtype = os.path.splitext(os.path.basename(pointer.group("path")))[0], "model"
            body.append(nxt)
            i += 1
        materializing = PATTERNS["materializing"].search(body[0])
        if materializing:
            node, rtype = materializing.group("name"), materializing.group("rtype")
        blocks.append(Finding(node, rtype, "\n".join(body).strip(), location=location))

    _attach(report, results, blocks, details)
    report.warnings = list(warned.values())
    report.dbt_success = summary_counts.get("success")
    _set_counts(report, seen, summary_counts, finished_errors, finished_warnings)
    return report


def _attach(report: Report, results: dict[str, Finding], blocks: list[Finding], details: dict[str, int]) -> None:
    """Merge failed result lines, `Test failed (N rows)` details and `[error]` blocks of the same node."""
    for name, rows in details.items():
        finding = results.get(name)
        if finding is not None:
            finding.failures = rows       # the row count is shown in the sentence; it is not a log message
    loose: list[Finding] = []
    for block in blocks:
        target = results.get(block.node) if block.node else None
        if target is not None:
            target.message = target.message or block.message
            target.location = target.location or block.location
            target.resource_type = target.resource_type or block.resource_type
        elif block.node:
            results[block.node] = block
        else:
            loose.append(block)
    report.errors = list(results.values()) + loose


def _set_counts(report: Report, seen: dict[str, int], summary: dict[str, int],
                finished_errors: int | None, finished_warnings: int | None) -> None:
    """Counts of TABLES AND CHECKS, from the result lines of the log. dbt's own `Summary:` also counts the hooks
    (`Processed: 1 hook`), so it is only used when the log holds no result lines at all, minus the hooks."""
    counts = dict(seen)
    if summary and not any(seen.values()):
        counts["pass"] = max(summary.get("success", 0) - report.hooks, 0)
        counts["warn"] = summary.get("warn", summary.get("warning", summary.get("warned", 0)))
        counts["error"] = summary.get("error", summary.get("errors", 0))
        counts["skip"] = summary.get("skipped", summary.get("skip", 0))
    for key, found in (("error", finished_errors), ("warn", finished_warnings)):
        if found is not None:
            counts[key] = max(counts[key], found)
    counts["error"] = max(counts["error"], len(report.errors))
    counts["warn"] = max(counts["warn"], len(report.warnings))
    counts["skip"] = max(counts["skip"], len(report.skipped))
    report.counts = counts


@dataclass
class Tally:
    """THE counts of one report, computed once and used by the subject, the summary line and the tables."""
    succeeded: int
    warned: int
    failed_checks: int
    failed_tables: int
    failed_other: int       # errors that name no table or check (connection, parse errors)
    not_run: int            # everything skipped: tables and checks together
    skipped_tables: int
    skipped_checks: int

    @property
    def failed(self) -> int:
        return self.failed_checks + self.failed_tables + self.failed_other


def tally(report: Report) -> Tally:
    checks = sum(1 for f in report.errors if f.resource_type == "test")
    tables = sum(1 for f in report.errors if f.node and f.resource_type != "test")
    failed = max(report.counts["error"], len(report.errors))
    skipped_tables = sum(1 for f in report.skipped if f.resource_type in TABLE_TYPES)
    not_run = max(report.counts["skip"], len(report.skipped))
    return Tally(report.counts["pass"], report.counts["warn"], checks, tables, failed - checks - tables, not_run,
                 skipped_tables, len(report.skipped) - skipped_tables)


# ---------------------------------------------------------------------------
# Readable names for dbt's generated test names
# ---------------------------------------------------------------------------

# (prefix, kind); longest prefixes first so `dbt_utils_source_unique_...` wins over `unique_`
_TEST_PREFIXES = sorted([
    ("dbt_utils_source_unique_combination_of_columns_", "combination"),
    ("dbt_utils_unique_combination_of_columns_", "combination"),
    ("dbt_utils_source_expression_is_true_", "expression"),
    ("dbt_utils_expression_is_true_", "expression"),
    ("source_not_null_", "not_null"), ("source_unique_", "unique"),
    ("source_accepted_values_", "accepted_values"), ("source_relationships_", "relationships"),
    ("not_null_", "not_null"), ("unique_", "unique"),
    ("accepted_values_", "accepted_values"), ("relationships_", "relationships"),
], key=lambda item: -len(item[0]))
# a generated name is `<table>_<column>`; the column is the last word, or the last two when that word is generic
_COLUMN_TAILS = {"id", "key", "hkey", "date", "dtm", "type", "status", "name", "code", "count", "amount", "number"}


def _split_table_column(text: str, is_source: bool) -> tuple[str, str]:
    """`raw_olist_raw_olist_orders_order_id` -> (`raw_olist_orders`, `order_id`). Best effort: dbt joins the parts
    with `_`, so the split is a heuristic (source name repeated up front; id-like column words), not exact."""
    tokens = text.split("_")
    if is_source:
        for size in range(1, len(tokens) // 2 + 1):
            if tokens[:size] == tokens[size:2 * size]:
                tokens = tokens[size:]
                break
    width = 2 if len(tokens) > 3 and tokens[-1] in _COLUMN_TAILS else 1
    if len(tokens) <= width:
        return "", "_".join(tokens)
    return "_".join(tokens[:-width]), "_".join(tokens[-width:])


def _parse_generated(name: str) -> dict[str, Any] | None:
    """kind, table, column, extra name parts and parent table of a generated test name, or None."""
    for prefix, kind in _TEST_PREFIXES:
        if not name.startswith(prefix):
            continue
        rest, is_source = name[len(prefix):], prefix.startswith(("source_", "dbt_utils_source_"))
        parts = rest.split("__")
        table, column = _split_table_column(parts[0], is_source)
        if not column:
            return None
        parent = parts[-1][4:].strip("_") if len(parts) > 1 and parts[-1].startswith("ref_") else ""
        return {"kind": kind, "table": table, "column": column, "extra": parts[1:], "parent": parent}
    return None


def describe_test(name: str) -> str | None:
    """A plain description of a test named by dbt's generator, or None for anything else (singular tests, custom
    names). Example: `source_not_null_raw_olist_raw_olist_orders_order_id` -> `order_id in raw_olist_orders must never
    be empty`."""
    parsed = _parse_generated(name)
    if parsed is None:
        return None
    kind, table, column, extra = parsed["kind"], parsed["table"], parsed["column"], parsed["extra"]
    where = f" in {table}" if table else ""
    if kind == "not_null":
        return f"{column}{where} must never be empty"
    if kind == "unique":
        return f"{column}{where} must be unique"
    if kind == "accepted_values":
        values = ", ".join(v.replace("_", " ") for v in extra)
        return f"{column}{where} must only hold the allowed values" + (f" ({values})" if values else "")
    if kind == "relationships":
        return f"every {column}{where} must exist in {parsed['parent']}" if parsed["parent"] else None
    if kind == "combination":
        return f"the combination of {', '.join([column] + extra)}{where} must be unique"
    if kind == "expression":
        rule = " ".join(extra)
        return f"{column}{where} must satisfy a rule" + (f" ({rule})" if rule else "")
    return None


def readable_name(name: str) -> str:
    """`assert_carrier_events_delivered` -> `carrier events delivered`."""
    words = re.sub(r"^(?:assert|test)_", "", name).replace("_", " ").strip()
    return words or name


def describe_failure(finding: Finding) -> str:
    """The first cell of a failure row, a plain sentence built from test kind, table, column and failed-row count:
    `3 rows in raw_olist_orders have no order_id`. Without a row count (the check itself broke) it says so."""
    n = finding.failures
    parsed = _parse_generated(finding.node) if finding.resource_type == "test" else None
    if finding.resource_type != "test":
        return describe_finding(finding)
    if n is None:
        return f"The check '{describe_test(finding.node) or readable_name(finding.node)}' did not finish"
    one = n == 1
    rows = _plural(n, "row")
    if parsed is None:
        text = readable_name(finding.node)
        return f"{text[:1].upper()}{text[1:]}: {rows}"
    kind, table, column, extra = parsed["kind"], parsed["table"], parsed["column"], parsed["extra"]
    in_table = f" in {table}" if table else ""
    if kind == "not_null":
        return f"{rows}{in_table} {'has' if one else 'have'} no {column}"
    if kind == "unique":
        return (f"{_plural(n, 'value')} of {column} {'appears' if one else 'appear'} more than once{in_table}")
    if kind == "accepted_values":
        return f"{rows}{in_table} {'has' if one else 'have'} {_article(column)} {column} that is not allowed"
    if kind == "relationships":
        parent = parsed["parent"] or "parent table"
        return f"{rows}{in_table} {'points' if one else 'point'} to a {parent} that does not exist"
    if kind == "combination":
        return (f"{_plural(n, 'combination')} of {', '.join([column] + extra)} "
                f"{'appears' if one else 'appear'} more than once{in_table}")
    rule = " ".join(extra)
    return f"{rows}{in_table} {'breaks' if one else 'break'} a rule on {column}" + (f" ({rule})" if rule else "")


def _article(word: str) -> str:
    return "an" if word[:1].lower() in "aeiou" else "a"


def describe_finding(finding: Finding, warned: bool = False) -> str:
    """`Check failed: order_id in raw_olist_orders must never be empty (6 rows)`; raw name as the fallback."""
    rows = "" if finding.failures is None else f" ({finding.failures} row{'' if finding.failures == 1 else 's'})"
    if finding.resource_type == "test":
        sentence = describe_test(finding.node)
        return f"Check {'warned' if warned else 'failed'}: {sentence or finding.node}{rows}"
    if finding.node:
        return f"{finding.resource_type.capitalize() or 'Node'} failed: {finding.node}"
    return "dbt reported an error"


# ---------------------------------------------------------------------------
# run_results.json
# ---------------------------------------------------------------------------


def load_run_results(path: str, not_before: float | None = None) -> dict[str, Any] | None:
    """The run_results.json at `path`, or None when missing, unreadable, or older than `not_before` (epoch seconds):
    a stale file from an earlier run must not describe this one."""
    try:
        if not path or not os.path.isfile(path):
            return None
        if not_before is not None and os.path.getmtime(path) < not_before:
            return None
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
        return data if isinstance(data, dict) else None
    except (OSError, ValueError):
        return None


def merge_run_results(report: Report, run_results: dict[str, Any] | None) -> Report:
    """Override / extend the log findings with run_results.json (status, message, failures, ...)."""
    results = (run_results or {}).get("results") or []
    if not results:
        return report
    errors = {f.node: f for f in report.errors}
    warnings = {f.node: f for f in report.warnings}
    counts = {"pass": 0, "warn": 0, "error": 0, "skip": 0}
    for res in results:
        status = str(res.get("status", "")).lower()
        unique_id = str(res.get("unique_id", ""))
        parts = unique_id.split(".")
        node = parts[2] if len(parts) >= 3 else _short_name(unique_id)
        if status in ("success", "pass", "passed"):
            counts["pass"] += 0 if parts[0] == "operation" else 1      # a hook is not a table or a check
            continue
        if status in ("skipped", "skip"):
            counts["skip"] += 1
            continue
        is_warn = status == "warn"
        counts["warn" if is_warn else "error"] += 1
        table = warnings if is_warn else errors
        finding = table.get(node) or Finding(node)
        table[node] = finding
        finding.unique_id = unique_id
        finding.resource_type = parts[0] if parts and parts[0] in RESOURCE_TYPES else finding.resource_type
        if res.get("message"):
            finding.message = str(res["message"])
        if res.get("failures") is not None:
            finding.failures = res["failures"]
        if res.get("execution_time") is not None:
            finding.duration = f"{float(res['execution_time']):.2f}s"
    report.errors, report.warnings, report.counts = list(errors.values()), list(warnings.values()), counts
    return report


# ---------------------------------------------------------------------------
# Rendering and sending
# ---------------------------------------------------------------------------

_TD = "padding:4px 8px;border:1px solid #ccc;vertical-align:top;"
_TH = "padding:4px 8px;border:1px solid #ccc;background:#f0f0f0;text-align:left;"


def _esc(value: Any) -> str:
    return html.escape(str(value if value is not None else ""))


def _table(headers: list[str], rows: list[list[str]]) -> str:
    head = "".join(f'<th style="{_TH}">{_esc(h)}</th>' for h in headers)
    body = "".join("<tr>" + "".join(f'<td style="{_TD}">{cell}</td>' for cell in row) + "</tr>" for row in rows)
    return f'<table style="border-collapse:collapse;font-size:13px;margin:8px 0;"><tr>{head}</tr>{body}</table>'


def _plural(count: int, word: str) -> str:
    return f"{count} {word}{'' if count == 1 else 's'}"


def _load_label(ctx: dict[str, Any]) -> str:
    return "Nightly load" if ctx.get("dag_id") in (None, "", "dbt_daily_load") else str(ctx["dag_id"])


ENVIRONMENT_NAMES = {"dev": "development", "val": "validation", "prd": "production"}


def _environment(ctx: dict[str, Any]) -> str:
    name = str(ctx.get("environment") or "?")
    return ENVIRONMENT_NAMES.get(name, name)


def render_subject(report: Report, ctx: dict[str, Any]) -> str:
    """`[development] Nightly load failed: 1 check failed, 6 steps not run`. Keep `Nightly load failed`: the
    workshop presenter app finds the mail by it."""
    t = tally(report)
    parts = []
    if t.failed_checks:
        parts.append(f"{_plural(t.failed_checks, 'check')} failed")
    if t.failed_tables:
        parts.append(f"{_plural(t.failed_tables, 'table')} failed")
    if t.failed_other:
        parts.append(f"{_plural(t.failed_other, 'error')}")
    if t.not_run:
        parts.append(f"{_plural(t.not_run, 'step')} not run")
    return f"[{_environment(ctx)}] {_load_label(ctx)} failed" + (": " + ", ".join(parts) if parts else "")


def summary_line(report: Report) -> str:
    """`42 succeeded, 1 failed, 6 not run` (plain text of the counts in the mail body; `warned` only when > 0)."""
    t = tally(report)
    parts = [f"{t.succeeded} succeeded"] + ([f"{t.warned} warned"] if t.warned else [])
    return ", ".join(parts + [f"{t.failed} failed", f"{t.not_run} not run"])


def count_note(report: Report) -> str:
    """Why dbt's own `Summary:` can show more successes than the mail: dbt counts the hooks as nodes."""
    if report.dbt_success is None or report.dbt_success == report.counts["pass"]:
        return ""
    extra = report.dbt_success - report.counts["pass"]
    if report.hooks and extra == report.hooks:
        return (f"dbt's own summary counts {report.dbt_success} succeeded because it also counts "
                f"{_plural(report.hooks, 'hook')} (a clean-up step after the run). This mail counts only tables and "
                f"checks, so it shows {report.counts['pass']}.")
    return (f"dbt's own summary counts {report.dbt_success} succeeded; this mail counts the "
            f"{report.counts['pass']} tables and checks that its log lists.")


def render_html(report: Report, ctx: dict[str, Any]) -> str:
    """ONE e-mail: header, counts, errors, warnings, skipped nodes, last log lines. Inline CSS only."""
    run = _esc(ctx.get("run_id", ""))
    if ctx.get("run_url"):
        run = f'<a href="{_esc(ctx["run_url"])}">{run}</a>'
    header = (
        f'<h2 style="margin:0 0 8px;color:#b00020;">{_esc(_load_label(ctx))} failed</h2>'
        f'<div style="font-size:13px;">Environment: <b>{_esc(_environment(ctx))}</b> &middot; '
        f'DAG: <b>{_esc(ctx.get("dag_id"))}</b> &middot; Run: {run} &middot; '
        f'Image tag: <b>{_esc(ctx.get("image_tag") or "unknown")}</b></div>'
    )
    t = tally(report)
    colors = {"pass": "#2e7d32", "warn": "#ed6c02", "error": "#b00020", "skip": "#616161"}
    shown = [("pass", t.succeeded, "succeeded")] + ([("warn", t.warned, "warned")] if t.warned else []) + [
        ("error", t.failed, "failed"), ("skip", t.not_run, "not run")]
    counts = ('<span style="display:inline-block;margin:8px 0;font-size:14px;">' + ", ".join(
        f'<span style="color:{colors[k]};"><b>{n}</b> {label}</span>' for k, n, label in shown) + "</span>")
    # explicit background: a dark-mode mail client would otherwise put the dark text on a dark page
    parts = [f'<div style="font-family:Arial,Helvetica,sans-serif;color:#222;background:#ffffff;padding:16px;'
             f'max-width:960px;">{header}<div>{counts}</div>']
    if report.summary or report.processed:
        stats = " / ".join(x for x in (report.summary, report.processed) if x)
        parts.append(f'<div style="font-size:12px;color:#555;">{_esc(stats)}</div>')
    if count_note(report):
        parts.append(f'<div style="font-size:12px;color:#555;margin-top:2px;">{_esc(count_note(report))}</div>')

    def message_cell(finding: Finding) -> str:
        text = finding.message[:MAX_MESSAGE_CHARS] + ("..." if len(finding.message) > MAX_MESSAGE_CHARS else "")
        return f'<pre style="margin:0;white-space:pre-wrap;font-size:12px;">{_esc(text)}</pre>' if text else ""

    def what_cell(finding: Finding, warned: bool = False) -> str:
        return f"<b>{_esc(describe_finding(finding, warned))}</b>"

    if report.errors:
        for f in report.errors:         # dbt's generic "Got N results, configured to fail" only repeats the count
            if re.match(r"^Got \d+ results?, configured to fail", f.message):
                f.message = ""
        with_details = any(f.message for f in report.errors)
        rows = [[f"<b>{_esc(describe_failure(f))}</b>", _esc(f.node), _esc(f.location)]
                + ([message_cell(f)] if with_details else []) for f in report.errors]
        parts.append("<h3>What failed</h3>" + _table(
            ["What happened", "Technical name", "File"] + (["Details"] if with_details else []), rows))
    if report.warnings:
        rows = [[what_cell(f, True), _esc(f.node), _esc(f.location)] for f in report.warnings]
        parts.append("<h3>Warnings</h3>" + _table(["What warned", "Technical name", "File"], rows))
    if report.skipped:
        tables = [f.node for f in report.skipped if f.resource_type in TABLE_TYPES]
        shown = ", ".join(tables[:MAX_SKIPPED_LISTED]) + (" ..." if len(tables) > MAX_SKIPPED_LISTED else "")
        parts.append(
            "<h3>Not run because something earlier failed</h3>"
            f'<div style="font-size:13px;">{_esc(_plural(t.not_run, "step"))} did not run: '
            f'{_esc(_plural(t.skipped_tables, "table"))} and {_esc(_plural(t.skipped_checks, "check"))}. '
            f"Not errors in themselves."
            + (f"<br>Tables: {_esc(shown)}" if tables else "") + "</div>")
    if report.tail:
        parts.append(
            f"<h3>Last {len(report.tail)} log lines</h3>"
            f'<pre style="background:#f6f6f6;color:#222;border:1px solid #ddd;padding:8px;font-size:11px;'
            f'white-space:pre-wrap;">'
            f"{_esc(chr(10).join(report.tail))}</pre>")
    parts.append("</div>")
    return "".join(parts)


def send_report(subject: str, html_body: str, env: dict[str, str] | None = None, smtp_factory=smtplib.SMTP) -> bool:
    """Send the mail through DBT_REPORT_SMTP_HOST / _PORT (no auth, no TLS). Never raises: returns False and logs."""
    env = os.environ if env is None else env
    try:
        host = env.get("DBT_REPORT_SMTP_HOST", "").strip()
        recipients = [r.strip() for r in env.get("DBT_FAILURE_EMAIL_RECIPIENTS", "").split(",") if r.strip()]
        sender = env.get("DBT_REPORT_MAIL_FROM", "").strip()
        if not (host and recipients and sender):
            logger.warning("Failure report not sent: DBT_REPORT_SMTP_HOST, DBT_REPORT_MAIL_FROM and "
                           "DBT_FAILURE_EMAIL_RECIPIENTS must all be set.")
            return False
        message = MIMEMultipart("alternative")
        message["Subject"], message["From"], message["To"] = subject, sender, ", ".join(recipients)
        message.attach(MIMEText(html_body, "html", "utf-8"))
        with smtp_factory(host, int(env.get("DBT_REPORT_SMTP_PORT", "25")), timeout=30) as smtp:
            smtp.sendmail(sender, recipients, message.as_string())
        logger.info("Failure report sent to %s", recipients)
        return True
    except Exception as exc:  # a broken mail path must not hide the original failure
        logger.error("Failure report could not be sent: %s", exc)
        return False


def build_and_send(log_text: str, ctx: dict[str, Any], run_results_path: str | None = None,
                   not_before: float | None = None, env: dict[str, str] | None = None) -> bool:
    """Parse, enrich from run_results.json when it belongs to this run, render and send. Never raises."""
    try:
        report = merge_run_results(parse_log(log_text), load_run_results(run_results_path or "", not_before))
        return send_report(render_subject(report, ctx), render_html(report, ctx), env=env)
    except Exception as exc:
        logger.error("Failure report could not be built: %s", exc)
        return False
