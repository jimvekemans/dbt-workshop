"""The release sequence in dags/dbt_release.py: a step that is handed to dbt_reload carries only keys that reach it.

Run from airflow/:  python -m pytest tests   (no Airflow installation needed: the DAG file is parsed, not imported)
"""
import ast
from pathlib import Path

DAG_FILE = Path(__file__).resolve().parents[1] / "dags" / "dbt_release.py"

# What `expand_release_steps` copies from a `defer_to_reload` step, and `_create_reload_trigger_task` sends on to dbt_reload.
# Any other key on such a step (a `vars` entry, say) is silently dropped.
RELOAD_STEP_KEYS = {
    "task_name",
    "defer_to_reload",
    "model",
    "threads",
    "batch_count",
    "start_date",
    "end_date",
    "limit",
    "run_tests",
    "cleanup_backup",
}


def release_steps() -> list[dict]:
    tree = ast.parse(DAG_FILE.read_text())
    for node in tree.body:
        if isinstance(node, ast.AnnAssign) and getattr(node.target, "id", None) == "RELEASE_STEPS":
            return ast.literal_eval(node.value)
    raise AssertionError("RELEASE_STEPS not found in dbt_release.py")


def test_a_reload_step_has_only_keys_that_reach_the_reload_dag():
    reload_steps = [s for s in release_steps() if s.get("defer_to_reload")]
    assert reload_steps, "the release sequence has no step handed to dbt_reload"
    for step in reload_steps:
        ignored = set(step) - RELOAD_STEP_KEYS
        assert not ignored, f"{step['task_name']}: {sorted(ignored)} are ignored (not forwarded to dbt_reload)"
