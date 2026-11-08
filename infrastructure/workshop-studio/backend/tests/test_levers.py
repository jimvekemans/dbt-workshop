"""The between-deploy-window levers (Airflow Variables), the generated feeds and the probes that observe them."""
import subprocess

from app import probes
from app.actions import airflow
from app.actions.base import Ctx
from app.engine import undo_actions, undo_label


def test_undo_covers_channels_variables_and_sql(engine):
    undo = undo_actions(engine.scenarios["S5_carrier_silent"])
    assert {"action": "set_channel", "env": "prod", "channel": "carrier", "delay_batches": 0} in undo
    # every Variable the scenario sets is deleted again - once per (env, key), whatever the steps did in between
    variables = [a for a in undo if a["action"] == "airflow_variable"]
    assert sorted(a["key"] for a in variables) == ["dbt_daily_load_dbtignore", "disabled_dbt_models"]
    assert all(a["delete"] and a["env"] == "prod" for a in variables)
    # a sql action is only undone when it says how
    assert not [a for a in undo if a["action"] == "sql"]
    e5 = undo_actions(engine.scenarios["E5_dev_dirty_data"])
    assert {"action": "sql", "env": "dev", "statement": "CALL sp_qa_cleanup()"} in e5
    assert [undo_label(a) for a in e5] == ["sql@dev", "dbt_env_vars@dev"]


def test_new_channel_faults_are_undone_to_healthy(engine):
    assert {"action": "set_channel", "env": "dev", "channel": "web", "bot_events": 0} in undo_actions(
        engine.scenarios["E8_bot_traffic_oom"])
    assert {"action": "set_channel", "env": "dev", "channel": "carrier", "late_pct": 0} in undo_actions(
        engine.scenarios["E6_late_scans_timeline"])


def test_airflow_variable_renders_json_and_deletes(settings):
    ctx = Ctx(settings=settings, dry_run=True)
    act = airflow.AirflowVariable()
    lines = act.render({"env": "both", "key": "disabled_dbt_models", "value": ["source:raw_carrier+"]}, ctx)
    assert len(lines) == 2 and lines[0].endswith("""disabled_dbt_models '["source:raw_carrier+"]'""")
    assert act.render({"env": "prod", "key": "dbt_env_vars", "delete": True}, ctx) == [
        "docker exec workshop-airflow-prod airflow variables delete dbt_env_vars"]


class RecordingCtx(Ctx):
    def __init__(self, *a, **kw):
        super().__init__(*a, **kw)
        self.calls = []

    def sh(self, argv, **kw):
        self.calls.append(list(argv))
        return subprocess.CompletedProcess(argv, 0, stdout="", stderr="")


def test_airflow_variable_sets_lists_as_json(settings):
    ctx = RecordingCtx(settings=settings)
    airflow.AirflowVariable().run({"env": "dev", "key": "dbt_env_vars", "value": {"DBT_SEVERITY_DEV_DATA": "error"}}, ctx)
    assert ctx.calls == [["docker", "exec", "workshop-airflow-dev", "airflow", "variables", "set", "dbt_env_vars",
                          '{"DBT_SEVERITY_DEV_DATA": "error"}']]


def test_reset_deletes_every_lever(settings, monkeypatch):
    from app.actions import reset
    done = []
    monkeypatch.setattr(reset, "execute", lambda name, p, ctx: done.append((name, p.get("key"))))
    monkeypatch.setattr(reset, "reset_branches", lambda ctx: None)
    monkeypatch.setattr(reset, "redeploy_baseline", lambda ctx: done.append(("redeploy", None)))
    monkeypatch.setattr(reset, "milestone_exists", lambda ctx, name: False)
    reset.Reset().run({}, Ctx(settings=settings))
    assert [k for n, k in done if n == "airflow_variable"] == list(airflow.LEVER_VARIABLES)
    assert ("redeploy", None) in done


def test_reset_restores_the_baseline_milestone_first(settings, monkeypatch):
    """With a `baseline` milestone the whole warehouse is restored (models, run log, grants) instead of re-seeding the
    landing tables only - and before anything else talks to SingleStore, because it restarts."""
    from app.actions import reset
    done = []
    monkeypatch.setattr(reset, "execute", lambda name, p, ctx: done.append((name, p.get("op"), p.get("name"))))
    monkeypatch.setattr(reset, "reset_branches", lambda ctx: done.append(("branches", None, None)))
    monkeypatch.setattr(reset, "redeploy_baseline", lambda ctx: None)
    monkeypatch.setattr(reset, "milestone_exists", lambda ctx, name: name == "baseline")
    out = reset.Reset().run({}, Ctx(settings=settings))
    assert done[0] == ("milestone", "restore", "baseline")
    assert not [d for d in done if d[0] in ("reset_landing", "reset_channels")]
    assert "milestone 'baseline'" in out.output


def test_milestone_action_calls_the_script(settings, tmp_path):
    from app.actions import milestones
    script = tmp_path / "milestones.sh"
    script.write_text("#!/bin/sh\n")
    settings.milestones_script = script
    ctx = RecordingCtx(settings=settings)
    milestones.Milestone().run({"op": "restore", "name": "baseline"}, ctx)
    assert ctx.calls == [["bash", str(script), "restore", "baseline"]]
    assert "stops SingleStore" in milestones.Milestone().render({"op": "create", "name": "x"}, ctx)[0]
    import pytest
    with pytest.raises(milestones.ActionError):
        milestones.Milestone().render({"op": "drop", "name": "x"}, ctx)
    with pytest.raises(milestones.ActionError):
        milestones.Milestone().render({"op": "restore"}, ctx)


def test_reset_redeploys_the_baseline_code(settings, tmp_path, monkeypatch):
    """A merged fix that tag_dev published to LDV must not survive a Reset (or the scenario has nothing to detect)."""
    import io
    import tarfile
    from app.actions import reset

    def archive(tag):
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as tar:
            for path, text in {"models/stg.sql": f"select 1 -- {tag}", "airflow/dags/dbt_daily_load.py": "# dag",
                               "airflow/utils/dbt_config.py": "# utils"}.items():
                data = text.encode()
                info = tarfile.TarInfo(f"dbt-vdp-{tag}-abc/{path}")
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data))
        return buf.getvalue()

    class FakeGitLab:
        def __init__(self, ctx):
            pass

        def p(self, path):
            return path

        def req(self, method, path, params=None, **kw):
            return type("R", (), {"content": archive(params["sha"])})()

    monkeypatch.setattr(reset, "GitLab", FakeGitLab)
    settings.workshop_state_dir = tmp_path
    for env in ("dev", "prod"):
        (tmp_path / env / "dbt" / "dbt_packages" / "dbt_utils").mkdir(parents=True)
        (tmp_path / env / "dbt" / "models").mkdir()
        (tmp_path / env / "dbt" / "models" / "fix_from_a_scenario.sql").write_text("select 2")
        (tmp_path / env / "dbt" / ".dbtignore").write_text("models/*/carrier/\n")
        (tmp_path / env / "dags" / "utils").mkdir(parents=True)
        (tmp_path / env / "dags" / "old_dag.py").write_text("# gone")
    assert reset.redeploy_baseline(Ctx(settings=settings)) == ["v1.0.0 -> dev", "2026_DROP1.0 -> prod"]
    dev = tmp_path / "dev"
    assert (dev / "dbt" / "models" / "stg.sql").read_text() == "select 1 -- v1.0.0"
    assert not (dev / "dbt" / "models" / "fix_from_a_scenario.sql").exists()
    assert not (dev / "dbt" / ".dbtignore").exists()
    assert (dev / "dbt" / "dbt_packages" / "dbt_utils").is_dir()           # installed by dbt deps: kept
    assert (dev / "dags" / "dbt_daily_load.py").exists() and not (dev / "dags" / "old_dag.py").exists()
    assert (dev / "dags" / "utils" / "dbt_config.py").exists()
    assert "2026_DROP1.0" in (tmp_path / "prod" / "dbt" / "models" / "stg.sql").read_text()


def test_only_the_newest_run_that_touched_a_node_counts():
    rows = [
        ("not_null_stg_carrier_tracking_events_v1_shipment_id", "fail", "2026-11-05 10:00:00", "test"),   # before the switch
        ("not_null_stg_carrier_tracking_events_v2_shipment_id", "pass", "2026-11-05 11:00:00", "test"),
        ("stg_carrier_tracking_events_v2", "success", "2026-11-05 11:00:00", "model"),
    ]
    out = probes.evaluate_tests(rows, {"dbt.stg": ["stg_carrier_tracking_events_v"]},
                                {"dbt.stg": ["stg_carrier_tracking_events_v1", "stg_carrier_tracking_events_v2"]})
    assert out["dbt.stg"]["health"] == "ok"


def test_model_results_count_for_model_nodes():
    rows = [("mart_web_journeys", "error", "2026-11-05 10:00:00", "model"),
            ("int_web_top_pages", "skipped", "2026-11-05 10:00:00", "model"),
            ("mart_web_journeys_extra_test", "fail", "2026-11-05 10:00:00", "test")]
    out = probes.evaluate_tests(rows, {}, {"dbt.j": ["mart_web_journeys"], "dbt.t": ["int_web_top_pages"]})
    assert out["dbt.j"]["health"] == "fail" and out["dbt.j"]["note"] == "failing: mart_web_journeys"
    assert out["dbt.t"]["health"] == "warn"    # skipped: not built this time
    # exact names for models: a test whose name merely contains the model name is not the model
    assert out["dbt.j"]["metrics"]["tests"] == 1


def test_daily_load_shows_the_newest_run_of_either_environment(settings, monkeypatch):
    class Alive(Ctx):
        def sh(self, argv, **kw):
            return subprocess.CompletedProcess(argv, 0, stdout="true", stderr="")
    runs = {"dev": [{"run_id": "studio__2", "state": "success", "start_date": "2026-11-05T11:00:00"}],
            "prod": [{"run_id": "studio__1", "state": "failed", "start_date": "2026-11-05T10:00:00"}]}
    monkeypatch.setattr(probes, "list_runs", lambda ctx, env, dag: runs[env] if dag == "dbt_daily_load" else [])
    monkeypatch.setattr(probes, "run_outcome", lambda ctx, env, dag, run: (run["state"], ""))
    monkeypatch.setattr(probes, "read_variables", lambda ctx, env: {"disabled_dbt_models": '["x"]'} if env == "prod" else {})
    out, _ = probes.probe_airflow(Alive(settings=settings), {"airflow.daily_load", "airflow.levers"})
    assert out["airflow.daily_load"]["health"] == "ok" and out["airflow.daily_load"]["metrics"]["env"] == "LDV"
    assert out["airflow.levers"]["health"] == "warn" and "PRD disabled_dbt_models" in out["airflow.levers"]["note"]


def test_e10_undo_restores_the_prd_service_account_idempotently(engine):
    """E10 hardens svc_prd_dbt by hand: clearing the scenario must put it back in grp_admin and drop the hand-made
    role/group - also after its own fix step re-granted the group (error 1917 = already in group)."""
    undo = undo_actions(engine.scenarios["E10_prd_hotfix_drift"])
    statements = [a["statement"] for a in undo]
    assert statements == ["DROP VIEW IF EXISTS im_dimensional.v_delivery_sla_hotfix", "DROP ROLE 'role_svc_prd'",
                          "DROP GROUP 'grp_svc_prd'", "GRANT GROUP 'grp_admin' TO 'svc_prd_dbt'@'%'"]
    assert undo[-1]["ignore_errors"] == [1917] and "ignore_errors" not in undo[0]


def test_sql_ignore_errors_counts_listed_codes_as_done(settings, monkeypatch):
    import pymysql
    from app.actions import sqlops

    class Cur:
        description = None
        def __enter__(self): return self
        def __exit__(self, *a): return False
        def execute(self, stmt): raise pymysql.err.InternalError(1917, 'User "svc_prd_dbt"@"%" is already in group')

    class Conn:
        def cursor(self): return Cur()
        def close(self): pass

    monkeypatch.setattr(sqlops, "connect", lambda ctx, db=None: Conn())
    act = sqlops.Sql()
    res = act.run({"env": "prod", "statement": "GRANT GROUP x", "ignore_errors": [1917]}, Ctx(settings=settings))
    assert "1917 ignored" in res.output
    import pytest
    with pytest.raises(pymysql.MySQLError):
        act.run({"env": "prod", "statement": "GRANT GROUP x"}, Ctx(settings=settings))
