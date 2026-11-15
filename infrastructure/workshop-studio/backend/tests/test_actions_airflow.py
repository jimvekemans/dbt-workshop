import re
from datetime import datetime
from pathlib import Path

import pydantic
import pytest

from studio.actions import airflow, clients
from studio.actions.base import ACTIONS, CLEANERS, StepFailed
from test_actions_support import make_ctx

DAGS = next(
    (
        p / "dbt-singlestore-workshop" / "airflow" / "dags"
        for p in Path(__file__).resolve().parents
        if (p / "dbt-singlestore-workshop").exists()
    ),
    Path("/nonexistent"),
)


def cli_recorder(monkeypatch, replies=None):
    calls = []

    def fake(ctx, env, *args, **kwargs):
        calls.append((env, args))
        return clients.Proc(0, (replies or {}).get(args[:2], ""), "")

    monkeypatch.setattr(clients, "airflow_cli", fake)
    return calls


@pytest.mark.skipif(not DAGS.exists(), reason="dbt project not mounted")
def test_every_static_task_id_of_the_dags_has_friendly_words():
    ids = set()
    for dag in DAGS.glob("*.py"):
        ids |= set(re.findall(r'task_id="([a-z_]+)"', dag.read_text()))
    assert ids and all(airflow.friendly_task(t) != "Working" for t in ids), sorted(
        t for t in ids if airflow.friendly_task(t) == "Working"
    )


def test_friendly_task_handles_groups_and_prefixes():
    assert airflow.friendly_task("batched_incremental_loads.batch_chunk_3") == "Loading one part of the new copy"
    assert airflow.friendly_task("dbt_build") == "Building all the reports"


def test_no_friendly_text_uses_jargon():
    words = " ".join(airflow.FRIENDLY_TASKS.values()) + " ".join(airflow.DAG_NAMES.values())
    assert not re.search(r"\b(DAG|MR|Variable|dbt)\b", words)


def test_set_variable_merges_dbt_env_vars(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch, {("variables", "get"): '{"A": "1", "B": "2"}'})
    result = airflow.set_variable(
        make_ctx(tmp_path), airflow.SetVariableParams(key="dbt_env_vars", value={"B": "9", "C": "3"})
    )
    sent = next(args for _, args in calls if args[:2] == ("variables", "set"))
    assert sent[3] == '{"A": "1", "B": "9", "C": "3"}'
    assert result.touched == {"variable:dev:dbt_env_vars"}
    assert result.summary == "Changed a setting of the nightly load in development."


def test_other_variables_are_replaced_and_lists_are_json(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    airflow.set_variable(
        make_ctx(tmp_path), airflow.SetVariableParams(key="dbt_daily_load_dbtignore", value=["a/*.sql"])
    )
    assert calls[-1][1] == ("variables", "set", "dbt_daily_load_dbtignore", '["a/*.sql"]')


def test_the_version_variable_is_not_settable_here():
    with pytest.raises(pydantic.ValidationError):
        airflow.SetVariableParams(key="dbt_image_tag", value="v1")


def test_delete_and_cleaner_remove_the_variable(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    ctx = make_ctx(tmp_path)
    assert airflow.delete_variable(ctx, airflow.DeleteVariableParams(key="disabled_dbt_models")).touched == {
        "variable:dev:disabled_dbt_models"
    }
    airflow.clean_variable(ctx, "variable:val:dbt_env_vars")
    assert calls[-1] == ("val", ("variables", "delete", "dbt_env_vars"))
    assert "variable" in CLEANERS


def test_run_state_uses_the_key_task_of_the_nightly_load():
    run = {"state": "success"}
    failed_build = [
        {"task_id": "dbt_build", "state": "failed"},
        {"task_id": "persist_artifacts_to_state", "state": "success"},
    ]
    assert airflow.run_state("dbt_daily_load", run, failed_build) == "failed"
    assert airflow.run_state("dbt_release", run, failed_build) == "success"
    assert airflow.run_state("dbt_release", run, [{"task_id": "audit_environment_drift", "state": "failed"}]) == "failed"
    assert airflow.run_state("dbt_daily_load", {"state": None}, []) == "queued"


TASKS_FAILED = [{"task_id": "audit_environment_drift", "state": "success"}, {"task_id": "dbt_build", "state": "failed"}]
TASKS_OK = [{"task_id": "dbt_build", "state": "success"}]


def test_judge_success_failure_and_expectations():
    ok = airflow.judge("dbt_daily_load", "dev", "success", "success", TASKS_OK, "r1")
    assert ok.summary == "The nightly load finished without problems in development." and ok.touched == {
        "models:dev",
        "mail",
    }
    expected = airflow.judge("dbt_daily_load", "dev", "failed", "failed", TASKS_FAILED, "r1")
    assert "as expected" in expected.summary and "Building all the reports" in expected.summary
    with pytest.raises(StepFailed) as err:
        airflow.judge("dbt_daily_load", "dev", "success", "failed", TASKS_FAILED, "r1")
    assert "Building all the reports" in err.value.summary
    with pytest.raises(StepFailed) as err:
        airflow.judge("dbt_daily_load", "dev", "failed", "success", TASKS_OK, "r1")
    assert "expected to fail" in err.value.summary


def test_run_dag_triggers_waits_and_shows_the_running_task(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    ctx = make_ctx(tmp_path)
    polls = iter([[{"task_id": "dbt_build", "state": "running"}], TASKS_OK])
    runs = iter([{"state": "running", "clear_number": 0}, {"state": "success", "clear_number": 0}])
    monkeypatch.setattr(airflow, "latest_run", lambda *a: next(runs))
    monkeypatch.setattr(airflow, "task_rows", lambda *a: next(polls))
    result = airflow.run_dag(ctx, airflow.RunDagParams(dag="dbt_release", conf={"dry_run": True}))
    verbs = [args[:2] for _, args in calls]
    assert verbs.count(("dags", "unpause")) == 2  # the release and the table rebuild it delegates to
    trigger = next(args for _, args in calls if args[:2] == ("dags", "trigger"))
    assert '"dry_run": true' in trigger[trigger.index("-c") + 1] and "-l" in trigger
    assert "Working on the release run: Building all the reports" in ctx.messages
    assert result.summary.startswith("The release run finished")


def test_trigger_failure_is_plain(tmp_path, monkeypatch):
    monkeypatch.setattr(
        clients,
        "airflow_cli",
        lambda ctx, env, *a, **k: clients.Proc(1 if a[:2] == ("dags", "trigger") else 0, "", "DagNotFound"),
    )
    with pytest.raises(StepFailed) as err:
        airflow.trigger(make_ctx(tmp_path), "dev", "dbt_daily_load", {})
    assert err.value.summary == "The nightly load could not be started in development."


def test_retry_failed_clears_the_latest_run_and_waits_for_the_restart(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    logical = datetime(2026, 3, 1, 4, 0, 0)
    states = iter(
        [
            {"run_id": "r1", "state": "failed", "logical_date": logical, "clear_number": 0},  # the run to retry
            {"run_id": "r1", "state": "failed", "logical_date": logical, "clear_number": 0},  # not restarted yet
            {"run_id": "r1", "state": "success", "logical_date": logical, "clear_number": 1},
        ]
    )
    monkeypatch.setattr(airflow, "latest_run", lambda *a: next(states))
    monkeypatch.setattr(airflow, "task_rows", lambda *a: TASKS_OK)
    result = airflow.retry_failed(make_ctx(tmp_path), airflow.RetryFailedParams(dag="dbt_daily_load"))
    clear = next(args for _, args in calls if args[:2] == ("tasks", "clear"))
    assert "-f" in clear and "2026-03-01T04:00:00+00:00" in clear
    assert result.summary == "The nightly load finished without problems in development."


def test_retry_without_an_earlier_run_fails_plainly(tmp_path, monkeypatch):
    monkeypatch.setattr(airflow, "latest_run", lambda *a: None)
    with pytest.raises(StepFailed):
        airflow.retry_failed(make_ctx(tmp_path), airflow.RetryFailedParams(dag="dbt_daily_load"))


def test_actions_are_registered_with_labels():
    for name in ("airflow.set_variable", "airflow.delete_variable", "airflow.run_dag", "airflow.retry_failed"):
        assert name in ACTIONS
    assert ACTIONS["airflow.run_dag"].label(airflow.RunDagParams(dag="dbt_daily_load")) == "Starting the nightly load"


def test_task_rows_keep_only_the_newest_try_of_each_task(monkeypatch, tmp_path):
    rows = [
        {"task_id": "dbt_build", "state": "failed", "start_date": None, "end_date": None, "try_number": 1},
        {"task_id": "send_failure_report", "state": "success", "start_date": None, "end_date": None, "try_number": 1},
        {"task_id": "dbt_build", "state": "success", "start_date": None, "end_date": None, "try_number": 2},
    ]
    monkeypatch.setattr(clients, "airflow_query", lambda *a, **k: rows)
    got = airflow.task_rows(make_ctx(tmp_path, "dev"), "dev", "dbt_daily_load", "r1")
    assert [(t["task_id"], t["state"]) for t in got] == [("send_failure_report", "success"), ("dbt_build", "success")]


def test_unused_slices_are_left_out_of_the_task_list():
    tasks = [{"task_id": "init_green_table", "state": "success"}] + [
        {"task_id": "reload__dummy_reload_date_window", "state": s} for s in ("success", "skipped", "skipped")
    ] + [{"task_id": "cleanup_scratch_table", "state": "skipped"}]
    labels = [(t["label"], t["state"]) for t in airflow.describe_tasks(tasks)]
    assert labels.count(("Rebuilding a large table without stopping readers", "skipped")) == 0
    assert ("Removing the unfinished copy", "skipped") in labels and len(labels) == 3
