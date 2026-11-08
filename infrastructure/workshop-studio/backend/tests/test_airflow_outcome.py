"""dbt_daily_load ends "success" even when its dbt_build task failed (persist_artifacts_to_state runs on all_done).
The probe and the trigger action must judge it by dbt_build, not by the run state."""
import subprocess

import pytest

from app.actions import airflow
from app.actions.base import ActionError, Ctx
from app import probes


class FakeCtx(Ctx):
    """docker CLI stand-in: `docker inspect` says running, `dags trigger` remembers the run id; sleep is instant."""
    def __init__(self, *a, cli_stdout: str = "", **kw):
        super().__init__(*a, **kw)
        self.calls: list[list[str]] = []
        self.run_id = None
        self.cli_stdout = cli_stdout

    def sh(self, argv, **kw):
        self.calls.append(list(argv))
        if "--run-id" in argv:
            self.run_id = argv[argv.index("--run-id") + 1]
        out = "true" if argv[:2] == ["docker", "inspect"] else self.cli_stdout
        return subprocess.CompletedProcess(argv, 0, stdout=out, stderr="")

    def sleep(self, seconds):
        pass


def _key_task(monkeypatch, state):
    calls = []

    def fake(ctx, env, dag, run_id, task_id):
        calls.append((dag, task_id))
        return state
    monkeypatch.setattr(airflow, "task_state", fake)
    return calls


def test_run_outcome_uses_the_key_task(settings, monkeypatch):
    ctx = Ctx(settings=settings)
    calls = _key_task(monkeypatch, "failed")
    state, note = airflow.run_outcome(ctx, "prod", "dbt_daily_load", {"run_id": "r", "state": "success"})
    assert state == "failed" and "dbt_build failed" in note and "ended success" in note
    assert calls == [("dbt_daily_load", "dbt_build")]
    _key_task(monkeypatch, "success")
    assert airflow.run_outcome(ctx, "prod", "dbt_daily_load", {"run_id": "r", "state": "success"}) == ("success", "")
    calls = _key_task(monkeypatch, "failed")
    assert airflow.run_outcome(ctx, "prod", "dbt_release", {"run_id": "r", "state": "success"}) == ("success", "")
    assert airflow.run_outcome(ctx, "prod", "dbt_daily_load", {"run_id": "r", "state": "running"}) == ("running", "")
    assert calls == []                      # no key task for dbt_release; a running run is not judged yet


def test_probe_shows_a_failed_build_as_failed(settings, monkeypatch):
    _key_task(monkeypatch, "failed")
    monkeypatch.setattr(probes, "list_runs", lambda ctx, env, dag: [
        {"run_id": f"{dag}__1", "state": "success", "start_date": "2026-11-05T07:00:00"}])
    out, status = probes.probe_airflow(FakeCtx(settings=settings), {"airflow.daily_load", "airflow.release"})
    assert out["airflow.daily_load"]["health"] == "fail" and "dbt_build failed" in out["airflow.daily_load"]["note"]
    assert out["airflow.release"]["health"] == "ok"
    assert status["airflow-prod"] == {"ok": True}


def _trigger(settings, monkeypatch, task, allow_failure):
    ctx = FakeCtx(settings=settings)
    _key_task(monkeypatch, task)
    monkeypatch.setattr(airflow, "list_runs", lambda c, env, dag: [{"run_id": ctx.run_id, "state": "success"}])
    return ctx, airflow.AirflowTrigger().run(
        {"env": "prod", "dag": "dbt_daily_load", "conf": {"manual_run_override": True}, "allow_failure": allow_failure}, ctx)


def test_trigger_fails_when_the_build_failed(settings, monkeypatch):
    with pytest.raises(ActionError, match="dbt_daily_load finished with state failed: dbt_build failed"):
        _trigger(settings, monkeypatch, "failed", allow_failure=False)


def test_trigger_reports_an_allowed_build_failure(settings, monkeypatch):
    _, res = _trigger(settings, monkeypatch, "failed", allow_failure=True)
    assert res.ok is False and "dbt_build failed" in res.output
    _, res = _trigger(settings, monkeypatch, "success", allow_failure=False)
    assert res.ok is True and res.output == "dbt_daily_load: success"


def test_task_state_falls_back_to_the_cli(settings, monkeypatch):
    monkeypatch.setattr(airflow, "_query_airflow_db", lambda *a, **k: None)   # metadata DB not reachable
    noisy = ('[warning  ] Could not import graphviz.\n'
             '[{"dag_id": "dbt_daily_load", "task_id": "dbt_build", "state": "failed"},'
             ' {"dag_id": "dbt_daily_load", "task_id": "persist_artifacts_to_state", "state": "success"}]\n')
    ctx = FakeCtx(settings=settings, cli_stdout=noisy)
    assert airflow.task_state(ctx, "prod", "dbt_daily_load", "r1", "dbt_build") == "failed"
    assert ctx.calls[-1][-6:] == ["tasks", "states-for-dag-run", "dbt_daily_load", "r1", "-o", "json"]
    assert airflow.task_state(ctx, "prod", "dbt_daily_load", "r1", "nope") is None
