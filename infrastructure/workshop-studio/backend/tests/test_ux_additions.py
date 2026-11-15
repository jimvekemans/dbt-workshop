"""The UX-review additions: honest outcomes, input help and marks, before/after, the `fix` box, file labels,
stopped steps, usual durations, the lean scenario response, recording types and run history order."""
from __future__ import annotations

import copy
import json
import threading
import time
from dataclasses import replace

import pytest
from fastapi.testclient import TestClient

from studio import engine as engine_module
from studio.actions import base
from studio.catalog import load_catalog
from studio.engine import CHECK_FAILED, Engine
from studio.main import create_app
from studio.models import Scenario
from studio.recorder import FrameCollector, Recorder, replay
from tests.conftest import TODAY, FakeFeeder, scenario, write_catalog
from tests.test_catalog import mutate, problems_of
from tests.test_engine import make_engine, open_demo, run, step_of, live, fast_replay  # noqa: F401

pytestmark = pytest.mark.usefixtures("fake_registry")

DAYV = {"day": "2026-03-06"}      # inside the range the fake feeder (TODAY) gives
CHECKED = {"check": "select count(*) from {{ db('edw_dv_bdv') }}.t", "good_when": "zero",
           "good": "All {{ value }} is fine.", "not_quite": "{{ value }} scans still cannot be read."}


def with_outcome(outcome) -> dict:
    return mutate(["demo", "steps", 0, "outcome"], outcome)


def fake_rows(monkeypatch, value=None, error=None):
    asked: list[tuple] = []

    def read_rows(settings, query, *, limit=200, timeout=5, user="root"):
        asked.append((query, limit, timeout, user))
        if error:
            raise error
        return ["n"], [(value,)]
    monkeypatch.setattr(engine_module.clients, "read_rows", read_rows)
    return asked


# --- A.1 outcome ---------------------------------------------------------------------------------------------------

def test_outcome_object_is_validated():
    ok = Scenario.model_validate(with_outcome(CHECKED))
    assert ok.demo.steps[0].outcome.is_good(0) and not ok.demo.steps[0].outcome.is_good(3)
    for bad in ({**CHECKED, "good_when": "negative"}, {k: v for k, v in CHECKED.items() if k != "check"},
                {k: v for k, v in CHECKED.items() if k != "not_quite"}, {**CHECKED, "good": "x" * 301},
                {**CHECKED, "extra": 1}):
        with pytest.raises(ValueError):
            Scenario.model_validate(with_outcome(bad))
    Scenario.model_validate(with_outcome({**CHECKED, "good": "{{ value }}" * 20 + "x" * 70}))   # 11 chars become 6


def test_outcome_problems_are_reported_by_the_catalog(settings, tmp_path):
    found = problems_of(settings, tmp_path, with_outcome({**CHECKED, "check": "delete from t"}))
    assert any("single read-only SELECT" in p for p in found)
    found = problems_of(settings, tmp_path, with_outcome({**CHECKED, "good": "{{ inputs.ghost }}"}))
    assert any("unknown input 'ghost'" in p for p in found)
    found = problems_of(settings, tmp_path, with_outcome({**CHECKED, "good": "{{ now() }}" * 27}))
    assert any("after filling in the templates" in p for p in found)
    found = problems_of(settings, tmp_path, with_outcome("{{ value }} things"))     # no number in a plain outcome
    assert any("only available in the sentences of an outcome check" in p for p in found)


@pytest.mark.parametrize("value, kind, text", [
    (0, "good", "All 0 is fine."), (7, "not_quite", "7 scans still cannot be read.")])
def test_check_picks_the_sentence_and_records_it(live, settings, monkeypatch, value, kind, text):
    live.catalog.scenarios["demo-one"].demo.steps[0].outcome = Scenario.model_validate(with_outcome(CHECKED)).demo.steps[0].outcome
    asked = fake_rows(monkeypatch, value)
    open_demo(live)
    step = step_of(run(live, live.run_step, "demo-one", "first", {}), "first")
    assert step["status"] == "done" and step["outcome"] == text and step["outcome_kind"] == kind
    assert asked == [("select count(*) from dev_edw_dv_bdv.t", 1, 5.0, "root")]
    stored = Recorder(settings.recordings_dir).load_step("demo-one", "first")
    assert stored["outcome"] == text and stored["outcome_kind"] == kind


def test_a_plain_outcome_is_good_and_a_failing_check_keeps_the_step_done(live, monkeypatch):
    open_demo(live)
    plain = step_of(run(live, live.run_step, "demo-one", "first", {}), "first")
    assert plain["outcome"] == "It worked." and plain["outcome_kind"] == "good"
    live.catalog.scenarios["demo-one"].demo.steps[0].outcome = Scenario.model_validate(with_outcome(CHECKED)).demo.steps[0].outcome
    fake_rows(monkeypatch, error=RuntimeError("database gone"))
    step = step_of(run(live, live.run_step, "demo-one", "first", {}), "first")
    assert step["status"] == "done" and step["outcome"] == CHECK_FAILED and step["outcome_kind"] == "not_quite"
    assert step["actions"][-1]["status"] == "failed" and "database gone" in step["actions"][-1]["details"]
    fake_rows(monkeypatch, value=None)                                   # no number at all is also an error
    step = step_of(run(live, live.run_step, "demo-one", "first", {}), "first")
    assert step["outcome"] == CHECK_FAILED and "not a number" in step["actions"][-1]["details"]


# --- A.2 to A.5 validation -----------------------------------------------------------------------------------------

TOGGLE = {"id": "flag", "type": "toggle", "label": "Flag", "default": False}
DAY = {"id": "when", "type": "day", "label": "Day", "default": "{{ day(-1) }}", "min": "{{ day(-7) }}", "max": "{{ day(0) }}"}


def test_input_help_and_mark():
    on_off = {"on": "It is on.", "off": "It is off."}
    ok = Scenario.model_validate(mutate(["demo", "steps", 0, "inputs"], [
        {**TOGGLE, "help": on_off}, {**DAY, "help": "Plain.", "mark": {"day": "{{ day(-4) }}", "label": "The change"}}]))
    assert ok.demo.steps[0].inputs[0].help.on == "It is on."
    for bad in ([{**DAY, "help": on_off}], [{**TOGGLE, "mark": {"day": "{{ day(-1) }}", "label": "x"}}],
                [{**DAY, "mark": {"day": "{{ day(-1) }}", "label": "x" * 41}}], [{**TOGGLE, "help": {"on": "only"}}]):
        with pytest.raises(ValueError):
            Scenario.model_validate(mutate(["demo", "steps", 0, "inputs"], bad))


def test_mark_day_must_be_inside_the_slider(settings, tmp_path):
    outside = [{**DAY, "mark": {"day": "{{ day(-30) }}", "label": "Too old"}}]
    assert any("outside the range" in p for p in problems_of(settings, tmp_path, mutate(["demo", "steps", 0, "inputs"], outside)))
    fine = [{**DAY, "mark": {"day": "{{ day(-4) }}", "label": "Changed"}}]
    folder = write_catalog(tmp_path / "good", [mutate(["demo", "steps", 0, "inputs"], fine)])
    catalog = load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    inputs = catalog.scenario_payload("demo-one", __import__("studio.templating", fromlist=["x"]).TemplateContext("dev", {}, lambda: TODAY))
    assert inputs["demo"]["steps"][0]["inputs"][0]["mark"] == {"day": "2026-03-06", "label": "Changed"}


def test_before_of_must_name_an_earlier_step_with_the_same_item(settings, tmp_path):
    def show(items):
        return mutate(["demo", "steps", 1, "show"], items)
    good = [{"type": "table", "title": "Rows", "query": "select 1", "before_of": "first"},
            {"type": "result", "title": "What happened"}]
    load_catalog(replace(settings, scenarios_dir=write_catalog(tmp_path / "ok", [show(good)])), lambda: TODAY)
    for item, message in (({"type": "table", "title": "R", "query": "select 1", "before_of": "second"}, "earlier step"),
                          ({"type": "table", "title": "R", "query": "select 1", "before_of": "ghost"}, "earlier step"),
                          ({"type": "result", "title": "R", "before_of": "first"}, "no result item at position 1")):
        assert any(message in p for p in problems_of(settings, tmp_path, show([item])))
    wrong_first = mutate(["demo", "steps", 0, "show"], [{"type": "table", "title": "R", "query": "select 1", "before_of": "first"}])
    assert any("earlier step" in p for p in problems_of(settings, tmp_path, wrong_first))
    second_position = show([{"type": "table", "title": "A", "query": "select 1"},
                            {"type": "table", "title": "B", "query": "select 1", "before_of": "first"}])
    assert any("no table item at position 2" in p for p in problems_of(settings, tmp_path, second_position))


def test_fix_state_only_on_flow_nodes_and_once():
    flow = copy.deepcopy(scenario()["problem"]["visual"])
    flow["nodes"][1]["state"] = "fix"
    Scenario.model_validate(mutate(["problem", "visual"], flow))
    flow["nodes"][2]["state"] = "fix"
    with pytest.raises(ValueError, match="at most one"):
        Scenario.model_validate(mutate(["problem", "visual"], flow))
    timeline = copy.deepcopy(scenario()["idea"]["visual"])
    timeline["days"][0]["state"] = "fix"
    with pytest.raises(ValueError):
        Scenario.model_validate(mutate(["idea", "visual"], timeline))


def test_file_label_is_optional_short_and_resolved(settings, tmp_path):
    block = scenario()["code"][0]
    with pytest.raises(ValueError):
        Scenario.model_validate(mutate(["code"], [{**block, "file_label": "x" * 61}]))
    folder = write_catalog(tmp_path / "labelled", [mutate(["code"], [{**block, "file_label": "The recipe"}])])
    catalog = load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    assert catalog.blocks["demo-one"][0]["file_label"] == "The recipe"
    assert "file_label" not in load_catalog(settings, lambda: TODAY).blocks["demo-one"][0]


# --- B: step state -------------------------------------------------------------------------------------------------

def test_rerunning_a_step_resets_every_later_step(live):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    state = run(live, live.run_step, "demo-one", "second", DAYV)
    assert step_of(state, "second")["status"] == "done" and step_of(state, "second")["evidence"][0]["data"]
    state = run(live, live.run_step, "demo-one", "first", {})
    later = step_of(state, "second")
    assert later["status"] == "todo" and later["summary"] == "" and later["outcome"] == "" and later["actions"] == []
    assert "outcome_kind" not in later and all(e["data"] is None for e in later["evidence"]) and later["evidence"]


def test_stopping_a_step_is_stopped_and_clears_later_steps(live, stack):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    run(live, live.run_step, "demo-one", "second", DAYV)
    live.catalog.scenarios["demo-one"].demo.steps[0].do[0] = {"action": "fake.block"}
    live.run_step("demo-one", "first", {})
    time.sleep(0.2)
    live.cancel()
    live.wait_idle(5)
    state = live.state()
    assert step_of(state, "first")["status"] == "stopped" and step_of(state, "first")["outcome"] == ""
    assert "outcome_kind" not in step_of(state, "first")
    second = step_of(state, "second")
    assert second["status"] == "todo" and second["outcome"] == "" and all(e["data"] is None for e in second["evidence"])


def test_replay_reset_and_stopped(live, make_engine, stack, fast_replay):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    run(live, live.run_step, "demo-one", "second", DAYV)
    eng = make_engine(mode="replay", s2_root_pwd="")
    run(eng, eng.prepare, "demo-one")
    run(eng, eng.run_step, "demo-one", "second", {})
    state = run(eng, eng.run_step, "demo-one", "first", {})
    assert step_of(state, "first")["outcome_kind"] == "good" and step_of(state, "second")["status"] == "todo"
    assert all(e["data"] is None for e in step_of(state, "second")["evidence"])
    assert all(e["recorded_at"] for e in step_of(state, "first")["evidence"])
    eng.catalog.scenarios["demo-one"].demo.steps[0].do[0] = {"action": "fake.block"}


# --- B: recordings -------------------------------------------------------------------------------------------------

def test_recording_stores_types_titles_and_recorded_at_per_step(live, settings):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    raw = json.loads((settings.recordings_dir / "demo-one.json").read_text())["steps"]["first"]
    assert raw["frames"][-1]["evidence"] == [{"type": "table", "title": "Rows", "data": {"rows": [["dev", "select 1"]]}}]
    assert raw["recorded_at"] and raw["show"] == ["table"] and len(raw["durations"]) == 1
    assert Recorder(settings.recordings_dir).load_step("demo-one", "first")["recorded_at"] == raw["recorded_at"]


def test_recording_that_no_longer_fits_the_show_list_counts_as_missing(live, make_engine, settings, fast_replay):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    rec = Recorder(settings.recordings_dir)
    assert rec.load_matching("demo-one", "first", ["table"]) is not None
    assert rec.load_matching("demo-one", "first", ["bars"]) is None
    assert rec.load_matching("demo-one", "first", ["table", "table"]) is None
    assert rec.current_steps("demo-one", {"first": ["table"], "second": ["table", "result"]}) == {"first"}
    eng = make_engine(mode="replay", s2_root_pwd="")
    eng.catalog.scenarios["demo-one"].demo.steps[0].show[0]["type"] = "bars"
    run(eng, eng.prepare, "demo-one")
    step = step_of(run(eng, eng.run_step, "demo-one", "first", {}), "first")
    assert step["status"] == "failed" and step["summary"] == "This step has not been recorded yet."


def test_old_recordings_without_types_still_play_and_match_by_count(tmp_path):
    rec = Recorder(tmp_path)
    old = {"scenario": "s", "recorded_at": "2026-01-01T00:00:00+00:00", "steps": {"a": {
        "inputs": {}, "summary": "x", "outcome": "y", "frames": [
            {"t": 0, "actions": [], "evidence": [None, {"rows": []}]}, {"t": 6.4, "actions": [], "evidence": [{"a": 1}, {"rows": []}]}]}}}
    (tmp_path / "s.json").write_text(json.dumps(old))
    step = rec.load_step("s", "a")
    assert step["frames"][1]["evidence"][0] == {"type": None, "title": None, "data": {"a": 1}}
    assert step["recorded_at"] == "2026-01-01T00:00:00+00:00" and step["outcome_kind"] == "good"
    assert rec.matches(step, ["table", "bars"]) and not rec.matches(step, ["table"])
    assert rec.usual_seconds("s", "a") == 6.4


def test_usual_seconds_is_the_median_of_the_kept_durations(tmp_path):
    rec = Recorder(tmp_path)
    assert rec.usual_seconds("s", "a") is None and rec.prepare_usual_seconds("s") is None
    rec.save_step("s", "a", {}, [], "", "", duration=10)
    assert rec.usual_seconds("s", "a") == 10
    for d in (30, 20):
        rec.save_step("s", "a", {}, [], "", "", duration=d)
    assert rec.usual_seconds("s", "a") == 20
    rec.save_prepare("s", 4)
    assert rec.prepare_usual_seconds("s") == 4
    rec.save_prepare("s", 8)
    assert rec.prepare_usual_seconds("s") == 6
    assert rec.step_ids("s") == {"a"}                                    # the preparation is not a step


def test_collector_stores_types_when_given():
    collector = FrameCollector(lambda: 0.0)
    collector.frame([], [{"n": 1}], [("table", "Rows")])
    assert collector.frames[0]["evidence"] == [{"type": "table", "title": "Rows", "data": {"n": 1}}]


def test_prepare_records_its_duration(live, settings):
    open_demo(live)
    assert Recorder(settings.recordings_dir).prepare_usual_seconds("demo-one") is not None


# --- B: run history ------------------------------------------------------------------------------------------------

def test_run_history_marks_the_run_of_this_step(live):
    @base.evidence("run_history", base.EVIDENCE["table"].params)
    def history(ctx, p):
        return {"runs": [{"started": "2000-01-01T00:00:00", "state": "good"},
                         {"started": "2999-01-01T00:00:00", "state": "good"}]}
    live.catalog.scenarios["demo-one"].demo.steps[0].show = [{"type": "run_history", "title": "Runs"}]
    open_demo(live)
    runs = step_of(run(live, live.run_step, "demo-one", "first", {}), "first")["evidence"][0]["data"]["runs"]
    assert [r.get("current") for r in runs] == [None, True]


def test_run_history_provider_returns_newest_first(monkeypatch):
    from studio import evidence
    rows = [("2026-03-01 10:00:00", 3, 0, 0, 1, 0, 0, 0), ("2026-03-05 10:00:00", 3, 0, 0, 1, 0, 1, 0), ("2026-03-03 10:00:00", 2, 0, 0, 0, 0, 0, 0)]
    monkeypatch.setattr(evidence, "rows_of", lambda ctx, query, user="root": ([], rows))
    ctx = base.Ctx(env="dev", settings=None, feeder=None)
    data = evidence.run_history.__wrapped__(ctx, evidence.RunHistoryParams()) if hasattr(evidence.run_history, "__wrapped__") \
        else evidence.run_history(ctx, evidence.RunHistoryParams())
    assert [r["started"] for r in data["runs"]] == ["2026-03-05 10:00:00", "2026-03-03 10:00:00", "2026-03-01 10:00:00"]


# --- B: the scenario response --------------------------------------------------------------------------------------

LEAKY = {"do", "setup", "query", "check", "dag", "match", "undo", "statement", "action", "notes_for", "env"}


def keys_anywhere(node) -> set:
    if isinstance(node, dict):
        return set(node) | {k for v in node.values() for k in keys_anywhere(v)}
    return {k for v in node for k in keys_anywhere(v)} if isinstance(node, list) else set()


def test_scenario_response_shows_only_what_the_ui_shows(settings, tmp_path):
    sc = mutate(["demo", "steps", 0, "outcome"], CHECKED)
    sc["demo"]["steps"][1]["show"][0]["before_of"] = "first"
    sc["demo"]["steps"][1]["show"][0]["caption"] = "On {{ inputs.day }}"
    app = create_app(replace(settings, scenarios_dir=write_catalog(tmp_path / "lean", [sc])), registry=False,
                     feeder_factory=lambda: FakeFeeder(None))
    with TestClient(app, base_url="http://localhost:8085") as client:
        body = client.get("/api/scenarios/demo-one").json()
    assert not keys_anywhere(body) & LEAKY
    text = json.dumps(body)
    for secret in ("select count", "select 1", "fake.set", "good_when", "stg_"):
        assert secret not in text
    steps = body["demo"]["steps"]
    assert set(steps[0]) == {"id", "title", "say", "button", "inputs", "show", "outcome", "usual_seconds"}
    assert steps[0]["show"] == [{"type": "table", "title": "Rows", "caption": None, "before_of": None}]
    assert steps[0]["outcome"] is None and steps[0]["usual_seconds"] is None and body["prepare_usual_seconds"] is None
    assert steps[1]["outcome"] == "Fixed." and steps[1]["show"][0]["before_of"] == "first"
    assert steps[1]["show"][0]["caption"].startswith("On 2026-")
    assert body["code"][0]["lines"] and body["problem"]["visual"]["type"] == "flow"


def test_scenario_response_carries_the_usual_seconds(live, settings, stack):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    app = create_app(settings, registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost:8085") as client:
        app.state.engine.wait_idle(10)
        body = client.get("/api/scenarios/demo-one").json()
        assert isinstance(body["demo"]["steps"][0]["usual_seconds"], float) and body["demo"]["steps"][1]["usual_seconds"] is None
        assert isinstance(body["prepare_usual_seconds"], float)
        recorded = client.get("/api/catalog").json()["groups"][0]["scenarios"][0]["recorded"]
        assert recorded is False                                          # the second step has no recording
