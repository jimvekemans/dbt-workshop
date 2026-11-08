import html
import logging
import os
import re
from airflow.utils.email import send_email
from utils.dbt_config import ENV_UPPER

logger = logging.getLogger(__name__)

# Default failure email recipients.
# Override at deployment time via the DBT_FAILURE_EMAIL_RECIPIENTS env var
# (comma-separated addresses), or via the 'failure_email_recipients' DAG param at trigger time.
_default_recipients_raw = os.getenv("DBT_FAILURE_EMAIL_RECIPIENTS", "jim.vekemans@vdab.be")
FAILURE_EMAIL_RECIPIENTS: list[str] = [
    r.strip() for r in _default_recipients_raw.split(",") if r.strip()
]
FAILURE_EMAIL_SUBJECT = "Airflow {ENV} alert: {dag_id}.{task_id} failed"
FAILURE_EMAIL_CONTENT = """
Task <b>{task_id}</b> in DAG <b>{dag_id}</b> failed.<br>
Run id: {run_id}<br>
Log URL: {log_url}
"""

TEST_PREFIXES = (
    "dbt_utils_unique_combination_of_columns_",
    "dbt_utils_",
    "relationships_",
    "accepted_values_",
    "not_null_",
    "unique_",
)


def normalize_dbt_log_line(line: str) -> str:
    """Strip Airflow wrapper prefixes so dbt output is easier to parse."""
    line = line.strip().strip("'\"")
    line = re.sub(r"^.*?\b(?:INFO|WARNING|ERROR)\s-\s", "", line)
    line = re.sub(r"^\[base\]\s*", "", line)
    line = re.sub(r"^\d{2}:\d{2}:\d{2}\s+", "", line)
    line = re.sub(r"^\[[^\]]+\]\s+\d{2}:\d{2}:\d{2}\s+", "", line)
    return line.replace("\\'", "'").replace('\\"', '"').strip()


def extract_model_from_test_name(test_name: str) -> str:
    """Extract target model name from dbt test convention."""
    clean_name = test_name
    for prefix in TEST_PREFIXES:
        if clean_name.startswith(prefix):
            clean_name = clean_name[len(prefix) :]
            break

    clean_name = clean_name.split("__")[0]
    model_match = re.search(
        r"^(.*?(?:_|^)(?:hb_incr|dhb|hb|ab|db|hf|df|dev))(?=_|$)", clean_name
    )
    if model_match:
        return model_match.group(1)

    parts = clean_name.split("_")
    if len(parts) >= 2 and parts[-1] == parts[-2]:
        return "_".join(parts[:-1])

    return clean_name


def extract_dbt_warning_details(log_text: str) -> list[dict]:
    """Extract test warnings, model names, and failed row counts from dbt logs."""
    log_text = log_text.replace("\\r\\n", "\n").replace("\\n", "\n")
    warnings, seen_tests = [], set()

    for raw_line in log_text.splitlines():
        normalized_line = normalize_dbt_log_line(raw_line)
        if not normalized_line:
            continue

        match = re.search(r"\bWARN\s+(\d+)\s+([^\s\[]+)", normalized_line)
        if match:
            failed_rows, test_name = match.group(1), match.group(2)
            if test_name not in seen_tests:
                seen_tests.add(test_name)
                warnings.append(
                    {
                        "model": extract_model_from_test_name(test_name),
                        "test": test_name,
                        "failed_rows": failed_rows,
                    }
                )

    return warnings


def extract_dbt_error_lines(log_text: str) -> list[str]:
    """Extract the dbt failure summary and model error block from logs."""
    log_text = log_text.replace("\\r\\n", "\n").replace("\\n", "\n")
    model_error_lines, result_lines = [], []
    summary_line, capture_model_block = None, False

    for raw_line in log_text.splitlines():
        normalized_line = normalize_dbt_log_line(raw_line)
        if not normalized_line:
            continue

        if summary_line is None and re.search(
            r"Completed with \d+ error", normalized_line
        ):
            summary_line = normalized_line[normalized_line.index("Completed") :]

        if "Failure in model" in normalized_line:
            capture_model_block = True
            if normalized_line not in model_error_lines:
                model_error_lines.append(normalized_line)
            continue

        if capture_model_block:
            if re.search(
                r"^(\d+\s+of\s+\d+\s+(?:START|OK|PASS|WARN|ERROR)|Done\. PASS=|Finished running|START hook:|Completed with)",
                normalized_line,
            ):
                capture_model_block = False
                continue

            if re.search(
                r"(Error in model|^\d{3,5}:|^compiled code at )", normalized_line
            ):
                if normalized_line not in model_error_lines:
                    model_error_lines.append(normalized_line)
                continue

            if re.search(
                r"(InsecureRequestWarning|Pod .* has phase|Deleting pod:|Task failed with exception|Traceback)",
                normalized_line,
            ):
                capture_model_block = False

    if summary_line:
        result_lines.append(summary_line)
    result_lines.extend(model_error_lines)
    return result_lines


def format_error_section(
    error_lines: list[str], warning_details: list[dict] = None
) -> str:
    """Build readable HTML for failure details and test warnings."""
    if not error_lines and not warning_details:
        return ""

    summary_line = next(
        (line for line in error_lines if line.startswith("Completed with ")), ""
    )
    detail_lines = [line for line in error_lines if line != summary_line]

    sections = ["<br><b>dbt Run Details</b><br>"]
    if summary_line:
        sections.append(f"<b>Summary</b><br><pre>{html.escape(summary_line)}</pre>")
    if detail_lines:
        sections.append(
            f"<b>Model Errors</b><br><pre>{html.escape(chr(10).join(detail_lines))}</pre>"
        )

    if warning_details:
        warn_lines = []
        for item in warning_details:
            warn_lines.extend(
                [
                    f"<b>Model: {html.escape(item['model'])}</b>",
                    f"  • Test: {html.escape(item['test'])}",
                    f"  • Failed rows: {html.escape(str(item['failed_rows']))}\n",
                ]
            )
        sections.append(
            f"<b>Test Warnings</b><br><pre>{chr(10).join(warn_lines).strip()}</pre>"
        )

    return "".join(sections)


def read_task_log_text(ti) -> str:
    """Read task logs from configured log reader with local fallback."""
    try:
        from airflow.utils.log.log_reader import TaskLogReader

        log_reader = TaskLogReader()
        if log_reader.supports_read:
            logs, _ = log_reader.read_log_chunks(ti, ti.try_number, {})
            if logs:
                chunks = [
                    chunk if isinstance(chunk, str) else "".join(map(str, chunk))
                    for chunk in logs
                ]
                return "\n".join(chunks)
    except Exception:
        pass

    log_filepath = getattr(ti, "log_filepath", None)
    if log_filepath and os.path.exists(log_filepath):
        try:
            with open(log_filepath, encoding="utf-8") as file:
                return file.read()
        except OSError:
            pass

    return ""



