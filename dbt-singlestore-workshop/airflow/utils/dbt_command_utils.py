"""dbt CLI command generator and argument parsing utilities.

Adheres to Single Responsibility Principle: handles all CLI command structuring,
flag formatting, test selection/severity overrides, selector mapping, reload chunking,
and bash quoting.
"""

import json
import re
import shlex
from datetime import datetime
from typing import Any

from utils.dbt_config import DAILY_ARTIFACTS_DIR, ON_DEMAND_ARTIFACTS_DIR, ON_DEMAND_TARGET_DIR, TARGET_DIR
from utils.dbt_runtime_overrides import parse_list


def format_cli_arg(val: Any) -> str:
    """Format a variable or argument string for safe CLI and bash execution.

    Preserves user-provided quoting and avoids extra string wrapping.
    """
    if val is None:
        return ""
    if isinstance(val, (int, float, bool)):
        return str(val)

    val_str = str(val).strip()
    if not val_str:
        return "''"

    # If the user already enclosed the value in single or double quotes, retain as-is
    if (val_str.startswith("'") and val_str.endswith("'")) or (
        val_str.startswith('"') and val_str.endswith('"')
    ):
        return val_str

    # Wrap in single quotes to pass cleanly through bash without shell expansion
    return f"'{val_str}'"


def format_dbt_bash_command(args: list[str]) -> list[str]:
    """Assemble a raw dbt argument list into an executable command for the dbt container:

    ["bash", "-c", "<formatted_command>"].

    Ensures flags like --vars and --args preserve exact quoting without extra wrapping.
    """
    formatted_parts: list[str] = []
    i = 0
    while i < len(args):
        token = str(args[i])
        if token in ("--vars", "--args") and i + 1 < len(args):
            formatted_parts.append(token)
            formatted_parts.append(format_cli_arg(args[i + 1]))
            i += 2
            continue

        # If token contains spaces and is not already quoted, quote it for bash
        if " " in token and not (
            (token.startswith("'") and token.endswith("'"))
            or (token.startswith('"') and token.endswith('"'))
        ):
            formatted_parts.append(f"'{token}'")
        else:
            formatted_parts.append(token)
        i += 1

    return ["bash", "-c", " ".join(formatted_parts)]


def build_exclude_args(
    exclude_models: str | list[str] | None = None,
    disabled_models: str | list[str] | None = None,
    is_scheduled: bool = False,
    execution_date: datetime | None = None,
) -> list[str]:
    """Assemble list of dbt model exclusion arguments.

    Args:
        exclude_models: User or step-defined exclude string (comma or space-separated) or list.
        disabled_models: System-disabled models from Airflow variables.
        is_scheduled: If True, automatically exclude 'tag:on-demand' (and 'tag:monthly' on non-first days).
        execution_date: Logical/execution date of the run; excludes 'tag:monthly' when day != 1.
    """
    excludes: list[str] = []

    def _add_tokens(raw: str | list[str] | None):
        # a JSON list ('["source:raw_carrier+", "int_web_top_pages"]') or a comma/newline/space separated string
        for token in parse_list(raw):
            if token not in excludes:
                excludes.append(token)

    _add_tokens(exclude_models)
    _add_tokens(disabled_models)

    if is_scheduled:
        if "tag:on-demand" not in excludes:
            excludes.append("tag:on-demand")
        if execution_date is not None and getattr(execution_date, "day", 1) != 1:
            if "tag:monthly" not in excludes:
                excludes.append("tag:monthly")

    return excludes


def build_daily_load_command(
    try_number: int = 1,
    state_dir: str = DAILY_ARTIFACTS_DIR,
    target_path: str = TARGET_DIR,
    threads: int | str | None = None,
    cli_vars: str | None = None,
    disabled_models: str | None = None,
    execution_date: datetime | None = None,
    as_bash_cmd: bool = True,
    use_selector: bool = True,
) -> list[str]:
    """Construct the dbt command for the daily production pipeline.

    - Uses the predefined dbt selector ('daily_load', or 'daily_load_monthly' on day 1)
      which encapsulates the model selection, unique_combination_of_columns test filter,
      indirect-selection=empty, and on-demand / monthly tag exclusions.
    - Applies CLI severity override to error level via --warn-error.
    - If try_number > 1, constructs 'dbt retry' referencing state_dir.
    - Formats command for bash execution if as_bash_cmd is True.
    """
    if try_number > 1:
        cmd = [
            "dbt",
            "retry",
            "--state",
            state_dir,
            "--target",
            "run",
            "--warn-error",
        ]
        if target_path:
            cmd.extend(["--target-path", target_path])
        return format_dbt_bash_command(cmd) if as_bash_cmd else cmd

    is_non_first_day = execution_date is not None and getattr(execution_date, "day", 1) != 1

    if use_selector:
        selector_name = (
            "daily_load_monthly"
            if (execution_date is not None and getattr(execution_date, "day", None) == 1)
            else "daily_load"
        )
        cmd = [
            "dbt",
            "build",
            "--selector",
            selector_name,
            "--warn-error",
        ]
        dynamic_excludes = build_exclude_args(disabled_models=disabled_models)
        if is_non_first_day and "tag:monthly" not in dynamic_excludes:
            dynamic_excludes.append("tag:monthly")
        if dynamic_excludes:
            cmd.extend(["--exclude", " ".join(dynamic_excludes)])
    else:
        cmd = [
            "dbt",
            "build",
            "--select",
            "resource_type:model test_name:unique_combination_of_columns test_name:dbt_utils.unique_combination_of_columns",
            "--indirect-selection=empty",
            "--warn-error",
        ]
        excludes = build_exclude_args(
            disabled_models=disabled_models,
            is_scheduled=True,
            execution_date=execution_date or datetime(2026, 1, 2),
        )
        if excludes:
            cmd.extend(["--exclude", " ".join(excludes)])

    if threads:
        cmd.extend(["--threads", str(threads)])

    if cli_vars and cli_vars.strip() and cli_vars.strip() != "{}":
        cmd.extend(["--vars", cli_vars.strip()])

    cmd.extend(["--target", "run"])
    if target_path:
        cmd.extend(["--target-path", target_path])

    return format_dbt_bash_command(cmd) if as_bash_cmd else cmd


# ==============================================================================
# On-demand build (dbt_on_demand)
# ==============================================================================

# What may appear in a dbt selection (tag:x, path:models/a/*, +model+, a,b, source:s.t, config.k:v, @model): no
# quotes, spaces, `$`, `;`, `|`, `&`, backticks. The values come from a form a person fills in.
_SELECTOR_TOKEN = re.compile(r"^[A-Za-z0-9_.:*+@,/-]+$")
_MODEL_NAME = re.compile(r"^[A-Za-z0-9_.-]+$")
# A selection that matches nothing (a typo) is only a warning for dbt: it builds nothing and exits 0, so the run looks
# fine. Asked for by hand, that must fail.
NO_NODES_IS_AN_ERROR = '{"error": ["NoNodesForSelectionCriteria", "NoNodesSelected"]}'


def _selector_tokens(label: str, raw: str | None) -> list[str]:
    """dbt selection syntax: separate arguments are 'either', a comma inside one argument is 'both'."""
    tokens = str(raw or "").split()
    for token in tokens:
        if not _SELECTOR_TOKEN.match(token):
            raise ValueError(f"{label}: {token!r} is not a dbt selector (allowed: letters, digits and _ . : * + @ , / -)")
    return tokens


def build_on_demand_command(
    select: str,
    exclude: str | None = None,
    full_refresh_models: str | list[str] | None = None,
    try_number: int = 1,
    state_dir: str = ON_DEMAND_ARTIFACTS_DIR,
    target_path: str = ON_DEMAND_TARGET_DIR,
) -> list[str]:
    """`dbt build --select <select> [--exclude <exclude>] [--vars '{full_refresh_models: [...]}'] --target run`.

    - `select` is required (an empty one would build the whole project), `exclude` is optional. A selection or
      exclusion that matches nothing fails the build (`--warn-error-options`), it does not "succeed" with nothing built.
    - `full_refresh_models` (list, or JSON / comma / space separated string) is passed as the dbt var of the same
      name, only when given; the project's models decide what they do with it.
    - try_number > 1 (a task that was cleared): `dbt retry` from the state dir of the previous attempt.
    Everything is shell-quoted: the values come from a form."""
    is_retry = try_number > 1
    if is_retry:
        retry = ["dbt", "retry", "--state", state_dir, "--target", "run"]
        if target_path:
            retry.extend(["--target-path", target_path])
        return ["bash", "-c", " ".join(shlex.quote(part) for part in retry)]

    select_tokens = _selector_tokens("select", select)
    if not select_tokens:
        raise ValueError("select must not be empty: dbt would build the whole project")
    cmd = ["dbt", "build", "--select", *select_tokens]
    exclude_tokens = _selector_tokens("exclude", exclude)
    if exclude_tokens:
        cmd.extend(["--exclude", *exclude_tokens])
    models = parse_list(full_refresh_models)
    for name in models:
        if not _MODEL_NAME.match(name):
            raise ValueError(f"full_refresh_models: {name!r} is not a model name")
    if models:
        cmd.extend(["--vars", "{full_refresh_models: [" + ", ".join(models) + "]}"])
    cmd.extend(["--warn-error-options", NO_NODES_IS_AN_ERROR, "--target", "run"])
    if target_path:
        cmd.extend(["--target-path", target_path])
    return ["bash", "-c", " ".join(shlex.quote(part) for part in cmd)]


# ==============================================================================
# Upstream Prep Command Builder (dbt_release)
# ==============================================================================


def extract_dbt_list_args(command: list[str] | str) -> list[str]:
    """Extract model selection and filtering arguments from a dbt command to pass to 'dbt list'.

    Always includes '--resource-type model' (or seed/snapshot when applicable) even when '-s'
    is present, ensuring test names are excluded from upstream $SELECT_EXPR resolution.
    """
    if isinstance(command, str):
        cmd_tokens = command.strip().split()
    else:
        cmd_tokens = []
        for token in command:
            cmd_tokens.extend(token.strip().split())

    if not cmd_tokens:
        return ["--resource-type", "model"]

    primary_op = (
        cmd_tokens[0]
        if cmd_tokens[0] != "dbt"
        else (cmd_tokens[1] if len(cmd_tokens) > 1 else "build")
    )
    start_idx = 1 if cmd_tokens[0] != "dbt" else 2

    list_args: list[str] = []
    has_resource_type = False
    i = start_idx
    while i < len(cmd_tokens):
        tok = cmd_tokens[i]
        if tok in ("-s", "--select", "--selector", "-m", "--models"):
            list_args.append(tok)
            i += 1
            while i < len(cmd_tokens) and not cmd_tokens[i].startswith("-"):
                list_args.append(cmd_tokens[i])
                i += 1
            continue
        elif tok in ("--exclude",):
            list_args.append(tok)
            i += 1
            while i < len(cmd_tokens) and not cmd_tokens[i].startswith("-"):
                list_args.append(cmd_tokens[i])
                i += 1
            continue
        elif tok in ("--resource-type", "--resource-types"):
            has_resource_type = True
            list_args.append(tok)
            i += 1
            while i < len(cmd_tokens) and not cmd_tokens[i].startswith("-"):
                list_args.append(cmd_tokens[i])
                i += 1
            continue
        i += 1

    if not has_resource_type:
        if primary_op == "seed":
            list_args.extend(["--resource-type", "seed"])
        elif primary_op == "snapshot":
            list_args.extend(["--resource-type", "snapshot"])
        else:
            list_args.extend(["--resource-type", "model"])

    return list_args


def build_upstream_prep_command(
    command: list[str] | str,
    threads: int | str | None = None,
    cli_vars: str | None = None,
    target_path: str | None = None,
    target: str = "run",
    as_bash_cmd: bool = True,
) -> list[str]:
    """Construct dynamic prep command to resolve initial targets via 'dbt list' and build their upstreams:

    dbt build -s +{nodes} --exclude {nodes} tag:on-demand
    """
    list_args = extract_dbt_list_args(command)
    list_args_cmd = [
        "dbt",
        "list",
        *list_args,
        "--target",
        target,
        "--output",
        "name",
        "--quiet",
    ]
    list_str = " ".join(list_args_cmd)

    build_flags: list[str] = ["--target", target]
    if threads:
        build_flags.extend(["--threads", str(threads)])
    if cli_vars and cli_vars.strip() and cli_vars.strip() != "{}":
        build_flags.extend(["--vars", format_cli_arg(cli_vars)])
    if target_path:
        build_flags.extend(["--target-path", target_path])

    flags_str = " ".join(build_flags)

    bash_script = (
        "set -e; "
        "echo '==> Resolving initial step target objects for upstream build prep...'; "
        f"TARGETS=$({list_str} 2>/dev/null | grep -E '^[a-zA-Z0-9_]+$' || true); "
        'if [ -z "$TARGETS" ]; then '
        "  echo 'No matching target objects found for upstream dependency build. Skipping prep.'; "
        "else "
        '  SELECT_EXPR=""; EXCLUDE_EXPR=""; '
        "  for node in $TARGETS; do "
        '    SELECT_EXPR="${SELECT_EXPR} +${node}"; '
        '    EXCLUDE_EXPR="${EXCLUDE_EXPR} ${node}"; '
        "  done; "
        '  echo "==> Building upstream dependencies for: $SELECT_EXPR (excluding: $EXCLUDE_EXPR)"; '
        f"  dbt build -s $SELECT_EXPR --exclude $EXCLUDE_EXPR tag:on-demand {flags_str}; "
        "fi"
    )

    return ["bash", "-c", bash_script] if as_bash_cmd else ["bash", "-c", bash_script]


# ==============================================================================
# Blue/Green Chunked Reload Command Builders
# ==============================================================================


# ---------------------------------------------------------------------------
# Reload: thin wrappers around the macros in macros/utils/reload_helpers.sql
# ---------------------------------------------------------------------------


def build_run_operation_command(macro: str, args: dict[str, Any], target: str = "run") -> list[str]:
    """dbt run-operation <macro> --args <json> --target <target> as a bash command."""
    cmd = ["dbt", "run-operation", macro, "--args", json.dumps(args), "--target", target]
    return format_dbt_bash_command(cmd)


def build_reload_init_command(model_name: str, target_path: str | None = None) -> list[str]:
    """Create the empty __new_<model> table and capture its compiled SQL (built-in dbt run)."""
    cmd = ["dbt", "run", "--select", model_name.strip(), "--full-refresh", "--target", "reload"]
    if target_path:
        cmd.extend(["--target-path", target_path])
    return format_dbt_bash_command(cmd)


def build_reload_batch_command(
    model_name: str,
    batch: int,
    batches: int,
    parts: int = 1,
    start_date: str | None = None,
    end_date: str | None = None,
    limit: int | None = None,
) -> list[str]:
    """reload_batch: batch `batch` of `batches`, split in `parts` INSERTs. Boundaries only."""
    args: dict[str, Any] = {"model_name": model_name.strip(), "batch": batch, "batches": batches, "parts": parts}
    for key, val in (("start_date", start_date), ("end_date", end_date), ("limit", limit)):
        if val is not None:
            args[key] = val
    return build_run_operation_command("reload_batch", args, "reload")


def build_reload_test_command(model_name: str, target_path: str | None = None) -> list[str]:
    """Built-in dbt test against __new_<model>."""
    cmd = ["dbt", "test", "--select", model_name.strip(), "--target", "reload"]
    if target_path:
        cmd.extend(["--target-path", target_path])
    return format_dbt_bash_command(cmd)


def build_reload_swap_command(model_name: str) -> list[str]:
    return build_run_operation_command("reload_atomic_swap", {"model_name": model_name.strip()}, "run")


def build_reload_cleanup_scratch_command(model_name: str) -> list[str]:
    return build_run_operation_command("reload_cleanup_scratch", {"model_name": model_name.strip()}, "run")


def build_reload_drop_backup_command(model_name: str) -> list[str]:
    return build_run_operation_command("reload_drop_backup", {"model_name": model_name.strip()}, "run")
