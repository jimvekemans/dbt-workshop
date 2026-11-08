"""Content validation + sim engine: every scenario validates, references real nodes, and its playbook ends green."""
import pytest

from app import actions
from app.actions.base import Ctx
from app.loader import ContentError, load_scenarios, load_world
from app.schema import HEALTH_RANK

STAR = {"H0_happy_sprint", "S1_kafka_late", "S3_drive_duplicate", "E1_release_collision", "C1_finance_fanout",
        "S5_carrier_silent", "S6_carrier_api_v2", "S7_web_customer_key", "E5_dev_dirty_data", "E6_late_scans_timeline",
        "E8_bot_traffic_oom", "E10_prd_hotfix_drift"}


def test_world_and_scenarios_load(settings):
    world = load_world(settings.scenarios_dir)
    scenarios = load_scenarios(settings.scenarios_dir, world)
    assert STAR <= set(scenarios)
    assert {s for s, sc in scenarios.items() if sc.star} == STAR
    assert len(world.sprint.days) == 10
    for gid in ("sources", "platform", "consumers"):
        assert gid in {g.id for g in world.groups}


def test_star_scenarios_are_fully_implemented(settings):
    scenarios = load_scenarios(settings.scenarios_dir, load_world(settings.scenarios_dir))
    for sid in STAR:
        sc = scenarios[sid]
        assert sc.status == "implemented"
        assert sc.steps, sid
        assert sc.live_actions(), f"{sid} has no live actions"
        assert all(s.explain for s in sc.steps), f"{sid}: every step needs an explanation"


def test_unknown_node_reference_is_rejected(settings, tmp_path):
    (tmp_path / "world.yaml").write_text((settings.scenarios_dir / "world.yaml").read_text())
    (tmp_path / "x.yaml").write_text("id: X\nlayer: all\ntitle: t\ninject: {narrative: n, sim: {nope.node: {health: fail}}}\n")
    world = load_world(tmp_path)
    with pytest.raises(ContentError, match="unknown node"):
        load_scenarios(tmp_path, world)


def test_every_live_action_renders(engine, settings):
    """Dry-run: commands are rendered (nothing executed) for every action of every scenario + sprint day."""
    ctx = Ctx(settings=settings, dry_run=True)
    acts = [a for sc in engine.scenarios.values() for a in sc.live_actions() + sc.requires]
    acts += [a for d in engine.world.sprint.days for a in d.live]
    assert acts
    for a in acts:
        lines = actions.render(a.action, a.params(), ctx)
        assert lines and all(isinstance(line, str) and line for line in lines), a
        res = actions.execute(a.action, a.params(), ctx)
        assert res.data.get("dry_run") is True


def test_patch_sets_referenced_by_scenarios_exist(engine, settings):
    from app.patches import load_patch
    for sc in engine.scenarios.values():
        for a in sc.live_actions():
            for key in ("patch", "resolution"):
                if a.params().get(key):
                    assert load_patch(settings.patches_dir, a.params()[key]).files or load_patch(settings.patches_dir, a.params()[key]).ops


def test_happy_sprint_ends_all_green(engine):
    for _ in range(10):
        engine.tick()
    snap = engine.snapshot()
    assert snap["sprintDay"] == 10
    assert snap["counts"]["ok"] == len(snap["nodes"]), {k: v["health"] for k, v in snap["nodes"].items() if v["health"] != "ok"}
    engine.tick()   # past the end is a no-op
    assert engine.snapshot()["sprintDay"] == 10


@pytest.mark.parametrize("sid", sorted(
    p.stem for p in __import__("pathlib").Path(__file__).resolve().parents[2].joinpath("scenarios").glob("*.yaml") if p.stem != "world"))
def test_playbook_ends_all_green_in_sim(engine, sid):
    snap = engine.inject(sid)
    sc = engine.scenarios[sid]
    if sc.impact:   # the injected problem is visible before the playbook runs
        assert any(HEALTH_RANK[n["health"]] >= HEALTH_RANK["warn"] for n in snap["nodes"].values()), sid
        assert snap["ripple"], "ripple (BFS depth from the injected nodes) drives the zoom-out animation"
    for i in range(len(sc.steps)):
        snap = engine.run_step(i)
    assert snap["phase"] == "resolved"
    bad = {k: (v["health"], v["note"]) for k, v in snap["nodes"].items() if v["health"] in ("warn", "fail")}
    assert not bad, f"{sid} leaves nodes unhealthy: {bad}"
    # step order is enforced
    with pytest.raises(ValueError):
        engine.run_step(0)


def test_sim_channel_knob_turns_channel_warn_and_back(engine):
    engine.tick()
    engine.set_knob("channel.kafka", "delay_batches", 2)
    assert engine.snapshot()["nodes"]["channel.kafka"]["health"] == "warn"
    engine.set_knob("channel.kafka", "enabled", False)
    assert engine.snapshot()["nodes"]["channel.kafka"]["health"] == "fail"
    engine.set_knob("channel.kafka", "enabled", True)
    engine.set_knob("channel.kafka", "delay_batches", 0)
    assert engine.snapshot()["nodes"]["channel.kafka"]["health"] == "ok"
    with pytest.raises(KeyError):
        engine.set_knob("channel.kafka", "nope", 1)


def test_state_survives_restart(engine, settings):
    from app.engine import Engine
    engine.tick(); engine.tick()
    engine.inject("S1_kafka_late")
    again = Engine(settings, engine.world, engine.scenarios)
    snap = again.snapshot()
    assert snap["sprintDay"] == 2 and snap["scenarioId"] == "S1_kafka_late"
    assert snap["nodes"]["landing.raw_olist_order_payments"]["health"] == "fail"


def test_live_mode_dry_run_renders_commands_without_executing(engine):
    engine.set_mode("live")
    engine.inject("S1_kafka_late")
    logs = [e["msg"] for e in engine.log_lines]
    assert any("[dry-run]" in m and "delay_batches" in m for m in logs)
    assert any("CALL analytics_prod.sp_trickle_olist_batch(7, 0)" in m for m in logs)


def test_probe_results_override_sim_health_only_in_live(engine):
    engine.set_observed({"channel.kafka": {"health": "fail", "note": "observed"}}, {})
    assert engine.snapshot()["nodes"]["channel.kafka"]["health"] != "fail"      # sim mode ignores observations
    engine.set_mode("live")
    node = engine.snapshot()["nodes"]["channel.kafka"]
    assert node["health"] == "fail" and node["observed"] and node["note"] == "observed"


def test_failed_live_action_leaves_map_and_step_untouched(engine, monkeypatch):
    """Live first, sim after: when the stack refuses, the map must not pretend the step happened."""
    from app.actions.base import Action, ActionError
    from app.schema import Scenario

    class Boom(Action):
        name = "boom"

        def render(self, p, ctx):
            return ["boom"]

        def run(self, p, ctx):
            raise ActionError("GitLab said no")

    monkeypatch.setitem(actions.REGISTRY, "boom", Boom())
    sc = Scenario.model_validate({
        "id": "T", "layer": "all", "title": "t", "inject": {"narrative": "n"},
        "steps": [{"title": "s", "live": [{"action": "boom"}], "sim": {"git.main": {"health": "fail"}}}],
    })
    engine.scenarios[sc.id] = sc
    engine.set_mode("live")
    engine.set_dry_run(False)
    engine.inject("T")
    before = engine.snapshot()["nodes"]["git.main"]["health"]
    with pytest.raises(ActionError):
        engine.run_step()
    snap = engine.snapshot()
    assert snap["stepIndex"] == 0 and snap["nodes"]["git.main"]["health"] == before
    assert "GitLab said no" in snap["error"] and snap["busy"] is None
    assert any(e["level"] == "error" and "GitLab said no" in e["msg"] for e in engine.log_lines)


def test_airflow_cli_json_is_found_behind_log_noise():
    from app.actions.airflow import _parse_json_list
    noisy = ('2026-11-04T21:34:01Z [warning  ] FileType is deprecated. [py.warnings] lineno=646\n'
             '[warning  ] Could not import graphviz.\n[{"dag_id": "d", "run_id": "r", "state": "success"}]\n')
    assert _parse_json_list(noisy) == [{"dag_id": "d", "run_id": "r", "state": "success"}]
    assert _parse_json_list("[warning ] nothing here") is None
    assert _parse_json_list("[]") == []


def test_pipeline_waiting_only_on_manual_jobs_counts_as_finished():
    from app.actions.gitlab import settled_status
    assert settled_status("manual", ["success", "manual"]) == "success"
    assert settled_status("manual", ["success", "failed", "manual"]) == "failed"
    assert settled_status("running", ["running"]) == "running"
    assert settled_status("success", ["success"]) == "success"
