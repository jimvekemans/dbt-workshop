"""Operational levers that change a dbt run WITHOUT a deploy (between two release windows).

All three are Airflow Variables, read when the task starts (not at DAG parse time):

``dbt_env_vars`` (JSON object, every dbt task of every DAG)
    Environment variables exported before dbt starts, e.g. ``{"DBT_CARRIER_V2_FROM": "2017-03-04 12:00:00"}``
    (carrier format switch-over moment) or ``{"DBT_SEVERITY_DEV_DATA": "error"}`` (dynamic test severity). Connection
    secrets and the deploy target cannot be overridden.

``dbt_daily_load_dbtignore`` (JSON list or newline/comma separated, dbt_daily_load only)
    Paths/globs appended to the project's ``.dbtignore`` (inside the throwaway container, in the image's project copy)
    before dbt starts, and removed again when it ends. The files
    disappear from the project: models, sources, tests, freshness checks and exposures in them are neither built, tested
    nor documented. Anything that refs an ignored node must be ignored as well, or the WHOLE project fails to parse.
    Use it when a feed must be out of the project until the next deploy window.

``disabled_dbt_models`` (JSON list or newline/comma/space separated, see ``dbt_command_utils.build_exclude_args``)
    Selectors appended to ``--exclude`` of the daily load. The nodes stay in the lineage (refs resolve, docs, state
    comparison, freshness) - they are just not executed. Downstream models keep reading the last good build.
"""

import json
import logging
import re
import shlex
from typing import Any

logger = logging.getLogger(__name__)

ENV_VARS_VARIABLE = "dbt_env_vars"
DBTIGNORE_VARIABLE = "dbt_daily_load_dbtignore"

_ENV_NAME = re.compile(r"^[A-Z][A-Z0-9_]*$")
# never from a Variable: connection secrets, and the target environment itself (a prd run must not think it is dev)
_PROTECTED_ENV = re.compile(r"^(SINGLESTORE_.*|IKL_HASH_SALT|DBT_DEPLOY_TARGET|DBT_PROFILES_DIR|DBT_TARGET_PATH|PATH|HOME)$")
_IGNORE_ENTRY = re.compile(r"^[A-Za-z0-9_./*!?\[\]-]+$")


def parse_list(raw: Any) -> list[str]:
    """A list from a JSON list (``'["a", "b"]'``) or a newline/comma/space separated string. Empty -> []."""
    if raw is None:
        return []
    if isinstance(raw, list):
        items = raw
    else:
        text = str(raw).strip()
        if not text:
            return []
        if text.startswith("["):
            try:
                items = json.loads(text)
            except ValueError as exc:
                raise ValueError(f"not a valid JSON list: {text[:80]!r} ({exc})") from exc
            if not isinstance(items, list):
                raise ValueError(f"expected a JSON list, got {type(items).__name__}")
        else:
            items = text.replace("\r", " ").replace("\n", " ").replace(",", " ").split()
    out: list[str] = []
    for item in items:
        tokens = parse_list(item) if isinstance(item, list) else (
            str(item).replace("\r", " ").replace("\n", " ").replace(",", " ").split())
        for token in tokens:
            if token not in out:
                out.append(token)
    return out


def parse_env_vars(raw: Any) -> dict[str, str]:
    """Validated ``dbt_env_vars``: a JSON object of NAME -> scalar. Invalid or protected names raise."""
    if raw is None or (isinstance(raw, str) and not raw.strip()):
        return {}
    data = raw if isinstance(raw, dict) else json.loads(raw)
    if not isinstance(data, dict):
        raise ValueError(f"{ENV_VARS_VARIABLE} must be a JSON object, got {type(data).__name__}")
    out: dict[str, str] = {}
    for name, value in data.items():
        if not _ENV_NAME.match(str(name)):
            raise ValueError(f"{ENV_VARS_VARIABLE}: invalid environment variable name {name!r}")
        if _PROTECTED_ENV.match(str(name)):
            raise ValueError(f"{ENV_VARS_VARIABLE}: {name} cannot be overridden from a Variable")
        if isinstance(value, (dict, list)):
            raise ValueError(f"{ENV_VARS_VARIABLE}: {name} must be a string or number")
        out[str(name)] = "" if value is None else str(value).lower() if isinstance(value, bool) else str(value)
    return out


def validate_ignore_entries(entries: list[str]) -> list[str]:
    for entry in entries:
        if not _IGNORE_ENTRY.match(entry) or ".." in entry or entry.startswith("/"):
            raise ValueError(f"{DBTIGNORE_VARIABLE}: {entry!r} is not a project-relative path or glob")
    return entries


def _bash_body(cmd: list[str] | str) -> tuple[str, bool]:
    """(shell body, was it a ['bash', '-c', body] command)."""
    if isinstance(cmd, str):
        return cmd, False
    if len(cmd) >= 3 and cmd[0] == "bash" and cmd[1] == "-c":
        return str(cmd[2]), True
    return " ".join(shlex.quote(str(c)) for c in cmd), False


def apply_overrides(cmd: list[str] | str, env_vars: dict[str, str] | None = None,
                    dbtignore: list[str] | None = None) -> list[str]:
    """Wrap a dbt command (``['bash', '-c', body]``, an argv list or a string) so the env vars are exported and the
    .dbtignore entries are in place while it runs. The project's own .dbtignore (if any) is restored afterwards (the
    container is thrown away anyway; the restore keeps the command usable in any writable project directory)."""
    body, _ = _bash_body(cmd)
    prefix: list[str] = []
    for name, value in (env_vars or {}).items():
        prefix.append(f"export {name}={shlex.quote(value)}")
    if dbtignore:
        lines = " ".join(shlex.quote(e) for e in validate_ignore_entries(dbtignore))
        prefix.extend([
            "if [ -f .dbtignore ]; then cp .dbtignore .dbtignore.airflow-orig; fi",
            "trap 'if [ -f .dbtignore.airflow-orig ]; then mv -f .dbtignore.airflow-orig .dbtignore; "
            "else rm -f .dbtignore; fi' EXIT",
            f"printf '%s\\n' {lines} >> .dbtignore",
            "echo \"dbt_daily_load_dbtignore: ignoring $(printf '%s ' " + lines + ")\"",
        ])
    if not prefix:
        return cmd if isinstance(cmd, list) else ["bash", "-c", body]
    return ["bash", "-c", "; ".join(prefix) + "; " + body]


def read_env_vars(get_variable) -> dict[str, str]:
    """``dbt_env_vars`` from Airflow. A broken value fails the task loudly instead of silently running without it."""
    env_vars = parse_env_vars(get_variable(ENV_VARS_VARIABLE, "{}"))
    if env_vars:
        logger.warning(f"{ENV_VARS_VARIABLE}: exporting {sorted(env_vars)} for this dbt run")
    return env_vars


def read_dbtignore(get_variable) -> list[str]:
    entries = validate_ignore_entries(parse_list(get_variable(DBTIGNORE_VARIABLE, "")))
    if entries:
        logger.warning(f"{DBTIGNORE_VARIABLE}: these files are left out of the project for this run: {entries}")
    return entries
