"""The medium/low review findings: request guard, scenario replace/clear, live-first knobs + reset, probe fence and
evidence, honest live checks, YAML hot reload, atomic state saves, prod review delivery, timeline live notes."""
import json
import shutil

import pytest
from fastapi.testclient import TestClient

from app import actions
from app.actions.base import Action, ActionError, Ctx, Result
from app.engine import undo_actions
from app.main import create_app, request_problem
from app.probes import evaluate_tests, latest_run
from app.schema import Scenario

ALLOWED = {"localhost", "127.0.0.1", "[::1]", "testserver"}


# ------------------------------------------------------------------------------------------- request guard
def test_request_problem_rules():
    ok = {"host": "localhost:8085", "x-studio": "1", "origin": "http://localhost:8085"}
    assert request_problem("POST", ok, ALLOWED) is None
    assert request_problem("POST", {**ok, "origin": "http://localhost:5173"}, ALLOWED) is None   # vite dev proxy
    assert request_problem("GET", {"host": "[::1]:8085"}, ALLOWED) is None
    assert "host" in request_problem("GET", {"host": "evil.example:8085"}, ALLOWED)            # DNS rebinding
    assert "cross-origin" in request_problem("POST", {**ok, "origin": "https://evil.example"}, ALLOWED)
    assert "X-Studio" in request_problem("POST", {"host": "localhost:8085"}, ALLOWED)          # plain form/fetch POST


def test_api_refuses_requests_a_foreign_page_could_send(settings):
    app = create_app(settings, start_probes=False)
    with TestClient(app) as c:
        assert c.post("/api/reset").status_code == 403                                    # no X-Studio header
        assert c.post("/api/step/next", headers={"X-Studio": "1", "Origin": "https://evil.example"}).status_code == 403
        assert c.get("/api/state", headers={"Host": "evil.example"}).status_code == 403
        assert c.get("/api/state").status_code == 200
        assert c.post("/api/tick", headers={"X-Studio": "1"}).json()["sprintDay"] == 1
        with pytest.raises(Exception):
            with c.websocket_connect("/ws", headers={"Origin": "https://evil.example"}) as ws:
                ws.receive_json()
        with c.websocket_connect("/ws", headers={"Origin": "http://localhost:8085"}) as ws:
            assert ws.receive_json()["type"] == "state"


# ------------------------------------------------------------------------------- scenario replace / clear
def test_replacing_a_scenario_rolls_back_its_map(engine):
    engine.inject("S1_kafka_late")
    assert engine.state["nodes"]["landing.raw_olist_order_payments"]["health"] == "fail"
    engine.inject("S3_drive_duplicate")
    nodes = engine.state["nodes"]
    assert nodes["landing.raw_olist_order_payments"]["health"] == "ok"          # S1's damage is gone
    assert nodes["channel.kafka"]["knobs"]["delay_batches"] == 0
    assert nodes["landing.raw_olist_order_reviews"]["health"] == "warn"         # S3 is in place


def test_clear_restores_the_map_from_before_the_injection(engine):
    engine.tick()
    engine.inject("S1_kafka_late")
    pre = engine.state["pre_inject"]          # captured after the injection's baseline (idle -> ok), before any effect
    assert pre["channel.kafka"]["health"] == "ok" and pre["channel.kafka"]["knobs"]["delay_batches"] == 0
    engine.tick()                             # sprint progress while the scenario is active is not rolled back
    engine.clear_scenario()
    nodes = engine.state["nodes"]
    for nid in ("channel.kafka", "landing.raw_olist_order_payments", "exposure.finance_revenue_report", "consumer.finance"):
        assert (nodes[nid]["health"], nodes[nid]["note"]) == (pre[nid]["health"], pre[nid]["note"]), nid
    assert nodes["channel.kafka"]["knobs"]["delay_batches"] == 0
    assert nodes["landing.raw_olist_order_payments"]["metrics"].get("lag_days") == pre["landing.raw_olist_order_payments"]["metrics"].get("lag_days")
    assert engine.state["scenario_id"] is None and engine.state["touched"] == [] and engine.state["sprint_day"] == 2


def test_undo_actions_reset_exactly_the_injected_fields(engine):
    assert undo_actions(engine.scenarios["S1_kafka_late"]) == [
        {"action": "set_channel", "env": "prod", "channel": "kafka", "delay_batches": 0}]
    assert undo_actions(engine.scenarios["S3_drive_duplicate"]) == [
        {"action": "set_channel", "env": "both", "channel": "drive", "duplicate_pct": 0}]
    assert undo_actions(engine.scenarios["H0_happy_sprint"]) == []


def _spy(monkeypatch, name, calls, fail=False):
    class Spy(Action):
        def render(self, p, ctx):
            return [f"{name} {p}"]

        def run(self, p, ctx):
            calls.append((name, p))
            if fail:
                raise ActionError(f"{name} refused")
            return Result(output="ok")
    Spy.name = name
    monkeypatch.setitem(actions.REGISTRY, name, Spy())


def _live(engine):
    engine.set_mode("live")
    engine.set_dry_run(False)


def test_live_faults_are_undone_on_clear_and_flagged_in_sim(engine, monkeypatch):
    calls: list = []
    _spy(monkeypatch, "set_channel", calls)
    engine.scenarios["F"] = Scenario.model_validate({
        "id": "F", "layer": "sources", "title": "fault", "inject": {"narrative": "n", "live": [
            {"action": "set_channel", "env": "prod", "channel": "kafka", "delay_batches": 2}]}})
    _live(engine)
    engine.inject("F")
    assert engine.snapshot()["liveFaultsPending"] == ["kafka@prod"]
    engine.set_mode("sim")
    engine.clear_scenario()                          # sim cannot undo the stack: it says so, the fault stays pending
    assert engine.snapshot()["liveFaultsPending"] == ["kafka@prod"]
    assert any("still set in the stack" in e["msg"] for e in engine.log_lines)
    engine.set_mode("live")
    engine.inject("F")
    calls.clear()
    engine.clear_scenario()
    assert calls == [("set_channel", {"env": "prod", "channel": "kafka", "delay_batches": 0})]
    assert engine.snapshot()["liveFaultsPending"] == []


def test_knob_is_live_first(engine, monkeypatch):
    _spy(monkeypatch, "set_channel", [], fail=True)
    _live(engine)
    with pytest.raises(ActionError):
        engine.set_knob("channel.kafka", "delay_batches", 3)
    assert engine.state["nodes"]["channel.kafka"]["knobs"]["delay_batches"] == 0   # SingleStore refused: unchanged


def test_reset_is_live_first_and_fences_the_probes(engine, monkeypatch):
    _spy(monkeypatch, "reset", [], fail=True)
    engine.tick()
    _live(engine)
    with pytest.raises(ActionError):
        engine.reset()
    assert engine.state["sprint_day"] == 1                                      # failed reset: map untouched
    _spy(monkeypatch, "reset", [])
    engine.reset()
    assert engine.state["sprint_day"] == 0 and engine.state["probe_since"]


# ----------------------------------------------------------------------------- probes + live step checks
def test_tests_without_results_are_not_observations():
    nodes = {"exposure.finance_revenue_report": ["assert_finance_revenue_reconciles"], "dbt.stg_olist_reviews": ["stg_olist_reviews"]}
    out = evaluate_tests([("unique_stg_olist_reviews_review_id", "fail")], nodes)
    assert out["dbt.stg_olist_reviews"]["health"] == "fail"
    assert "exposure.finance_revenue_report" not in out           # no result for its test: no made-up "ok"
    assert evaluate_tests([("unique_stg_olist_reviews_review_id", "pass")], nodes)["dbt.stg_olist_reviews"]["health"] == "ok"


def test_airflow_runs_from_before_the_reset_are_ignored():
    runs = [{"run_id": "old", "state": "failed", "start_date": "2026-11-05T06:00:00"},
            {"run_id": "new", "state": "success", "start_date": "2026-11-05T07:00:00"}]
    assert latest_run(runs)["run_id"] == "new"
    assert latest_run(runs[:1], since="2026-11-05 06:30:00") is None
    assert latest_run([*runs, {"run_id": "queued", "state": "queued", "start_date": None}], "2026-11-05 06:30:00")["run_id"] == "queued"


def test_live_expectation_needs_an_observation(engine):
    engine.scenarios["X"] = Scenario.model_validate({
        "id": "X", "layer": "all", "title": "t", "inject": {"narrative": "n"},
        "steps": [{"title": "a", "sim": {"exposure.marketing_nps_dashboard": {"health": "ok"}},
                   "expect": {"node": "exposure.marketing_nps_dashboard", "health": "ok"}},
                  {"title": "b", "expect": {"node": "ci.verify", "health": "ok"}}]})
    _live(engine)
    engine.inject("X")
    engine.run_step()                              # the step itself set the sim health: that must not count
    assert any("not verifiable live" in e["msg"] for e in engine.log_lines)
    engine.state["observed"] = {"ci.verify": {"health": "ok"}}
    engine.run_step()
    assert any(e["msg"] == "✓ expectation: ci.verify is ok" for e in engine.log_lines)


# --------------------------------------------------------------------------------- YAML reload, state file
def test_editing_the_yaml_and_refreshing_reloads_it(settings, tmp_path):
    scen = tmp_path / "scenarios"
    shutil.copytree(settings.scenarios_dir, scen)
    settings.scenarios_dir = scen
    app = create_app(settings, start_probes=False)
    with TestClient(app, headers={"X-Studio": "1"}) as c:
        v0 = c.get("/api/world").json()["state"]["contentVersion"]
        p = scen / "S1_kafka_late.yaml"
        p.write_text(p.read_text().replace('title: "Kafka payments arrive late"', 'title: "Kafka payments arrive VERY late"'))
        w = c.get("/api/world").json()
        assert w["state"]["contentVersion"] == v0 + 1 and w["contentError"] is None
        assert any(s["title"] == "Kafka payments arrive VERY late" for s in w["scenarios"])
        p.write_text("id: [broken")
        w = c.get("/api/world").json()
        assert w["contentError"] and "S1_kafka_late" in w["contentError"]
        assert any(s["title"] == "Kafka payments arrive VERY late" for s in w["scenarios"])   # last good content stays


def test_state_file_is_written_atomically(engine, settings):
    engine.tick()
    engine.inject("S1_kafka_late")
    data = json.loads(settings.state_file.read_text())
    assert data["scenario_id"] == "S1_kafka_late" and data["pre_inject"]
    assert not list(settings.state_file.parent.glob(".*.tmp"))


# ------------------------------------------------------------------------------- trickle + timeline notes
def test_prod_trickle_renders_the_review_rule(settings):
    lines = actions.render("trickle", {"env": "both", "days": 1}, Ctx(settings=settings, dry_run=True))
    assert lines[0] == "CALL analytics_dev.sp_trickle_olist_batch(1, 1);"
    assert lines[1].startswith("CALL analytics_prod.sp_trickle_olist_batch(1, 0);") and "until PRD has received" in lines[1]
    assert actions.render("trickle", {"env": "prod", "days": 1, "domain_b": True}, Ctx(settings=settings))[0].endswith("(1, 1);")


def test_live_tick_says_what_is_only_simulated(engine):
    engine.set_mode("live")                          # dry-run: the trickle is only rendered
    engine.tick()
    engine.tick()
    assert any(e["msg"].startswith("Live: only the trickle runs") for e in engine.log_lines)
    assert all(d.live_note for d in engine.world.sprint.days[1:9])
