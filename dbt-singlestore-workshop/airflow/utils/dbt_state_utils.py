"""dbt state management and artifact processing utilities.

Adheres to Single Responsibility Principle: handles manifest and run_results
parsing, and state accumulation across sequential tasks.
"""

import json
import logging
import os
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
