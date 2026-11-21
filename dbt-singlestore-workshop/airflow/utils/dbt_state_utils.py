"""dbt state management and artifact processing utilities.

Adheres to Single Responsibility Principle: handles manifest and run_results
parsing, and state accumulation across sequential tasks.
"""

import json
import logging
import os
import re
import shutil
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


def _normalize_reload_relation_fields(obj: dict[str, Any]) -> None:
    """Normalize __new_ prefixes in relation_name and alias after a Blue/Green swap."""
    rel = obj.get("relation_name")
    if isinstance(rel, str) and "__new_" in rel:
        obj["relation_name"] = rel.replace("__new_", "")
    alias = obj.get("alias")
    if isinstance(alias, str) and alias.startswith("__new_"):
        obj["alias"] = alias[len("__new_") :]


def merge_run_results_and_manifest(
    source_dir: str,
    accumulated_state_dir: str,
) -> dict[str, Any]:
    """Merge source dbt execution artifacts (run_results.json and manifest.json)

    into an accumulated state directory.

    Returns the merged run_results dictionary.
    """
    source_run_results_path = os.path.join(source_dir, "run_results.json")
    source_manifest_path = os.path.join(source_dir, "manifest.json")

    os.makedirs(accumulated_state_dir, exist_ok=True)
    accumulated_run_results_path = os.path.join(
        accumulated_state_dir, "run_results.json"
    )
    accumulated_manifest_path = os.path.join(accumulated_state_dir, "manifest.json")

    # 1. Update manifest.json (latest task's manifest represents current compilation graph)
    if Path(source_manifest_path).is_file():
        try:
            with open(source_manifest_path, "r", encoding="utf-8") as mf:
                manifest_data = json.load(mf)
            for node_info in manifest_data.get("nodes", {}).values():
                if isinstance(node_info, dict):
                    _normalize_reload_relation_fields(node_info)
            temp_manifest_path = accumulated_manifest_path + ".tmp"
            with open(temp_manifest_path, "w", encoding="utf-8") as mf_out:
                json.dump(manifest_data, mf_out)
            os.replace(temp_manifest_path, accumulated_manifest_path)
        except Exception:
            temp_manifest_path = accumulated_manifest_path + ".tmp"
            shutil.copy(source_manifest_path, temp_manifest_path)
            os.replace(temp_manifest_path, accumulated_manifest_path)
        logger.info(f"Updated accumulated manifest.json from {source_manifest_path}")

    if not Path(source_run_results_path).is_file():
        logger.warning(
            f"No run_results.json found at {source_run_results_path}, skipping state merge."
        )
        if Path(accumulated_run_results_path).is_file():
            with open(accumulated_run_results_path, "r", encoding="utf-8") as f:
                return json.load(f)
        return {}

    with open(source_run_results_path, "r", encoding="utf-8") as f:
        source_run_results = json.load(f)

    for res in source_run_results.get("results", []):
        if isinstance(res, dict):
            _normalize_reload_relation_fields(res)

    # 2. Merge run_results.json
    if Path(accumulated_run_results_path).is_file() and os.path.abspath(
        source_run_results_path
    ) != os.path.abspath(accumulated_run_results_path):
        with open(accumulated_run_results_path, "r", encoding="utf-8") as f:
            accumulated_run_results = json.load(f)

        merged_results_by_id = {
            res["unique_id"]: res
            for res in accumulated_run_results.get("results", [])
            if "unique_id" in res
        }
        for res in source_run_results.get("results", []):
            if "unique_id" in res:
                merged_results_by_id[res["unique_id"]] = res

        accumulated_run_results["results"] = list(merged_results_by_id.values())

        if "elapsed_time" in source_run_results:
            accumulated_run_results["elapsed_time"] = (
                accumulated_run_results.get("elapsed_time", 0.0)
                + source_run_results["elapsed_time"]
            )

        # Update metadata and args to align with the latest manifest invocation
        if "metadata" in source_run_results:
            accumulated_run_results["metadata"] = source_run_results["metadata"]
        if "args" in source_run_results:
            accumulated_run_results["args"] = source_run_results["args"]
    else:
        accumulated_run_results = source_run_results

    temp_results_path = accumulated_run_results_path + ".tmp"
    with open(temp_results_path, "w", encoding="utf-8") as f:
        json.dump(accumulated_run_results, f, indent=2)
    os.replace(temp_results_path, accumulated_run_results_path)

    logger.info(
        f"Successfully merged results from {source_dir} into {accumulated_run_results_path}"
    )
    return accumulated_run_results


def consolidate_reload_artifacts(target_path: str) -> dict[str, Any]:
    """Consolidate isolated step artifacts (init, batch_1..N, test) into target_path root.

    Ensures the final target_path/run_results.json retains both the model execution
    results ('model.*') and the quality test results ('test.*') for downstream
    dbt_release '--state' consumption.
    """
    if not os.path.isdir(target_path):
        return {}

    subdirs: list[str] = []
    init_dir = os.path.join(target_path, "init")
    if os.path.isdir(init_dir):
        subdirs.append(init_dir)

    batch_dirs = sorted(
        [
            os.path.join(target_path, d)
            for d in os.listdir(target_path)
            if d.startswith("batch_") and os.path.isdir(os.path.join(target_path, d))
        ],
        key=lambda p: (
            int(os.path.basename(p).split("_")[1])
            if os.path.basename(p).split("_")[1].isdigit()
            else 0
        ),
    )
    subdirs.extend(batch_dirs)

    test_dir = os.path.join(target_path, "test")
    if os.path.isdir(test_dir):
        subdirs.append(test_dir)

    merged: dict[str, Any] = {}
    for sub in subdirs:
        if os.path.isfile(os.path.join(sub, "run_results.json")):
            merged = merge_run_results_and_manifest(
                source_dir=sub,
                accumulated_state_dir=target_path,
            )

    return merged


# ---------------------------------------------------------------------------
# Refreshed tables (the asset events of dbt_daily_load)
# ---------------------------------------------------------------------------

_QUOTES = re.compile(r"[`\"\[\]]")


def _relation_of(result: dict[str, Any], node: dict[str, Any] | None) -> tuple[str, str]:
    """(database, table) of a model result: its `relation_name` (`db`.`table`), else database and alias of the
    manifest node. ("", "") when neither says."""
    relation = result.get("relation_name") or (node or {}).get("relation_name") or ""
    parts = [p for p in _QUOTES.sub("", str(relation)).split(".") if p]
    if len(parts) >= 2:
        return parts[0], parts[-1]
    if node and node.get("database") and (node.get("alias") or node.get("name")):
        return str(node["database"]), str(node.get("alias") or node["name"])
    return "", ""


def _number(value: Any) -> int | float | None:
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def select_refreshed_tables(
    run_results: dict[str, Any], manifest: dict[str, Any] | None = None
) -> list[dict[str, Any]]:
    """The tables a build really refreshed: models with status `success` in run_results.json.

    Left out: everything that is not a model, every model that failed, errored or was skipped (a failed build's
    unbuilt tables), and - when the manifest is at hand - a model that has a test with status fail/error (it was built,
    but it failed its own checks). Result: [{database, table, unique_id, status, rows_affected, execution_time}].
    """
    results = [r for r in (run_results.get("results") or []) if isinstance(r, dict)]
    nodes = (manifest or {}).get("nodes") or {}
    held_back: set[str] = set()
    for res in results:
        unique_id = str(res.get("unique_id", ""))
        if unique_id.split(".")[0] in ("test", "unit_test") and str(res.get("status", "")).lower() in ("fail", "error"):
            held_back.update(((nodes.get(unique_id) or {}).get("depends_on") or {}).get("nodes") or [])
    tables: list[dict[str, Any]] = []
    for res in results:
        unique_id = str(res.get("unique_id", ""))
        if not unique_id.startswith("model.") or str(res.get("status", "")).lower() != "success":
            continue
        if unique_id in held_back:
            continue
        database, table = _relation_of(res, nodes.get(unique_id))
        if not database or not table:
            continue
        rows = _number((res.get("adapter_response") or {}).get("rows_affected"))
        tables.append({
            "database": database,
            "table": table,
            "unique_id": unique_id,
            "status": "success",
            "rows_affected": int(rows) if rows is not None else None,
            "execution_time": _number(res.get("execution_time")),
        })
    return tables


def extract_refreshed_tables(
    run_results_path: str, manifest_path: str | None = None, not_before: float | None = None
) -> dict[str, Any]:
    """{"invocation_id": ..., "tables": select_refreshed_tables(...)} of the build in `run_results_path`.

    Raises FileNotFoundError (the task is then skipped) when run_results.json is missing, or older than `not_before`
    (epoch seconds: the start of this DAG run): that file is from an earlier run, not from this one.
    """
    if not os.path.isfile(run_results_path):
        raise FileNotFoundError(f"Missing run_results file: {run_results_path}")
    if not_before is not None and os.path.getmtime(run_results_path) < not_before:
        raise FileNotFoundError(f"{run_results_path} is older than this run (left by an earlier run): nothing built")
    with open(run_results_path, "r", encoding="utf-8") as handle:
        run_results = json.load(handle)
    manifest = None
    if manifest_path and os.path.isfile(manifest_path):
        try:
            with open(manifest_path, "r", encoding="utf-8") as handle:
                manifest = json.load(handle)
        except ValueError:
            logger.warning(f"Could not read {manifest_path}: tests are not checked against the models")
    return {
        "invocation_id": (run_results.get("metadata") or {}).get("invocation_id"),
        "tables": select_refreshed_tables(run_results, manifest),
    }
