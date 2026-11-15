from __future__ import annotations

import json
import threading
import time

import pytest

from studio import engine as engine_module
from studio.actions import base
from studio.catalog import load_catalog
from studio.engine import BadRequest, Broker, Conflict, Engine, TouchedStore, leftover_label
from studio.recorder import Recorder, replay
from tests.conftest import FakeFeeder, TODAY


pytestmark = pytest.mark.usefixtures("fake_registry")


@pytest.fixture
def make_engine(settings, stack):
    engines: list[Engine] = []

    def build(**overrides) -> Engine:
        from dataclasses import replace
        cfg = replace(settings, **overrides)
        eng = Engine(cfg, load_catalog(cfg, lambda: TODAY), Recorder(cfg.recordings_dir), lambda: FakeFeeder(stack))
        engines.append(eng)
        return eng

    yield build
    stack.gate.set()
    for eng in engines:
        eng.cancel()
        eng.wait_idle(5)


@pytest.fixture
def live(make_engine):
    return make_engine()


def run(eng: Engine, op, *args):
    op(*args)
    eng.wait_idle(10)
    return eng.state()


def step_of(state, step_id):
    return next(s for s in state["demo"]["steps"] if s["id"] == step_id)


def open_demo(eng: Engine, sid: str = "demo-one"):
    state = run(eng, eng.prepare, sid)
    assert state["demo"]["status"] == "ready", state["demo"]
    return state


def test_prepare_builds_baseline_once_then_runs_setup(live, stack):
    state = open_demo(live)
    for env in ("dev", "val", "prd"):       # the environments are checked at the same time: only each one's order is fixed
        mine = [c for c in stack.calls if c.endswith(f":{env}:90") or c.endswith(f":{env}:v1.0.0") or c == f"build:{env}"]
        assert mine == [f"seed:{env}:90", f"deploy:{env}:v1.0.0", f"build:{env}"]
    assert sum(c.startswith(("seed", "deploy", "build")) for c in stack.calls) == 9
    assert state["baseline"]["status"] == "ready" and all(p["status"] == "done" for p in state["baseline"]["progress"])
    assert stack.calls[-1] == "set:ready" and state["job"] is None
    open_demo(live)                                                     # second time: nothing to fix
    assert sum(c.startswith(("seed", "deploy", "build")) for c in stack.calls) == 9


def test_step_runs_actions_shows_evidence_and_records(live, stack, settings):
    open_demo(live)
    state = run(live, live.run_step, "demo-one", "first", {})
    step = step_of(state, "first")
    assert step["status"] == "done" and step["outcome"] == "It worked." and step["summary"] == "Set one."
    assert step["actions"][0]["status"] == "done" and step["evidence"][0]["data"] == {"rows": [["dev", "select 1"]]}
    assert state["leftovers"] == [{"label": "A setting changed in development", "from_scenario": "demo-one"}]
    recorded = Recorder(settings.recordings_dir).load_step("demo-one", "first")
    assert recorded["outcome"] == "It worked." and recorded["frames"][-1]["evidence"][0]["data"]["rows"]


def test_inputs_are_validated_templated_and_remembered(live, stack):
    open_demo(live)
    with pytest.raises(BadRequest):
        live.run_step("demo-one", "second", {"pick": "z"})
    with pytest.raises(BadRequest):
        live.run_step("demo-one", "second", {"ghost": 1})
    with pytest.raises(BadRequest):
        live.run_step("demo-one", "second", {"day": "2020-01-01"})
    state = run(live, live.run_step, "demo-one", "second", {"pick": "b", "day": "2026-03-08"})
    step = step_of(state, "second")
    assert step["evidence"][0]["query"] == "select '2026-03-08'"
    assert step["evidence"][1]["data"] == {"summary": "Set two."}       # `result` evidence sees the last action


def test_step_needs_an_open_demo_in_live_mode(live):
    with pytest.raises(Conflict, match="Open the demo first"):
        live.run_step("demo-one", "first", {})
    with pytest.raises(BadRequest):
        live.prepare("nope")


def test_failed_step_is_plain_and_later_steps_stay_todo(live):
    live.catalog.scenarios["demo-one"].demo.steps[0].do[0] = {"action": "fake.fail"}
    open_demo(live)
    state = run(live, live.run_step, "demo-one", "first", {})
    assert step_of(state, "first")["status"] == "failed"
    assert step_of(state, "first")["summary"] == "The load did not finish."
    assert step_of(state, "second")["status"] == "todo" and state["demo"]["status"] == "ready"


def test_unexpected_error_becomes_a_plain_sentence_with_traceback_in_details(live):
    live.catalog.scenarios["demo-one"].demo.steps[0].do[0] = {"action": "fake.boom"}
    open_demo(live)
    step = step_of(run(live, live.run_step, "demo-one", "first", {}), "first")
    assert step["summary"] == "Something went wrong talking to fake."
    assert "RuntimeError: kaput" in step["actions"][0]["details"]


def test_evidence_error_only_marks_its_own_item(live):
    live.catalog.scenarios["demo-one"].demo.steps[0].show.append({"type": "number", "title": "Broken"})
    open_demo(live)
    evidence = step_of(run(live, live.run_step, "demo-one", "first", {}), "first")["evidence"]
    assert "rows" in evidence[0]["data"] and evidence[1]["data"] == {"error": engine_module.EVIDENCE_ERROR}


def test_cancel_inside_a_wait(live):
    live.catalog.scenarios["demo-one"].demo.steps[0].do[0] = {"action": "fake.sleep"}
    open_demo(live)
    live.run_step("demo-one", "first", {})
    time.sleep(0.2)
    assert live.state()["job"]["kind"] == "step"
    live.cancel()
    live.wait_idle(5)
    state = live.state()
    assert step_of(state, "first")["status"] == "stopped" and "Stopped" in step_of(state, "first")["summary"]
    assert state["job"] is None


def test_second_job_is_refused_while_one_runs(live, stack):
    live.catalog.scenarios["demo-one"].demo.steps[0].do[0] = {"action": "fake.block"}
    open_demo(live)
    live.run_step("demo-one", "first", {})
    for op, args in ((live.prepare, ("demo-two",)), (live.cleanup, ()), (live.run_step, ("demo-one", "second", {}))):
        with pytest.raises(Conflict, match="still running"):
            op(*args)
    stack.gate.set()
    live.wait_idle(5)
    assert step_of(live.state(), "first")["status"] == "done"


def test_cleanup_goes_in_clean_order_and_empties_the_set(live, stack):
    store = live._store
    store.add({"models:dev", "variable:dev:b", "landing:dev", "git", "feed:dev:carrier", "variable:dev:a"}, "demo-one")
    state = run(live, live.cleanup)
    cleaned = [c.removeprefix("clean:") for c in stack.calls if c.startswith("clean:")]
    assert cleaned == ["variable:dev:a", "variable:dev:b", "feed:dev:carrier", "git", "landing:dev", "models:dev"]
    assert state["leftovers"] == [] and state["notice"]["text"] == "Everything is back to normal."


def test_cleanup_progress_is_visible_as_actions_of_the_job(live, stack):
    live._store.add({"variable:dev:a"}, "demo-one")
    stack.hold_cleaners = release = threading.Event()   # the cleaner stays busy until the test has looked
    try:
        live.cleanup()
        deadline = time.monotonic() + 5
        labels: list[str] = []
        while time.monotonic() < deadline and not labels:
            job = live.state()["job"]
            labels = [a["label"] for a in job["actions"]] if job else []
            time.sleep(0.01)
        assert labels == ["Putting back: A setting changed in development"]
        assert live.state()["job"]["actions"][0]["status"] == "running"
    finally:
        release.set()
    live.wait_idle(5)
    assert live.state()["job"] is None


def test_failing_cleaner_keeps_its_resource_and_is_reported(live, stack):
    stack.failing_cleaners.add("landing")
    live._store.add({"variable:dev:a", "landing:dev"}, "demo-one")
    state = run(live, live.cleanup)
    assert state["leftovers"] == [{"label": "Demo data delivered to development", "from_scenario": "demo-one"}]
    assert "Demo data delivered to development" in state["notice"]["text"] and state["notice"]["level"] == "problem"


def test_touched_set_survives_a_restart(live, make_engine, settings):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    saved = json.loads((settings.data_dir / "state.json").read_text())
    assert set(saved["touched"]) == {"variable:dev:ready", "variable:dev:one"}
    again = make_engine()
    assert {item["label"] for item in again.state()["leftovers"]} == {"A setting changed in development"}
    assert TouchedStore(settings.data_dir / "state.json").items() == saved["touched"]


def test_prepare_cleans_another_scenarios_leftovers_first(live, stack):
    open_demo(live, "demo-one")
    run(live, live.run_step, "demo-one", "first", {})
    stack.calls.clear()
    state = open_demo(live, "demo-two")
    first_clean = next(i for i, c in enumerate(stack.calls) if c.startswith("clean:"))
    assert first_clean < stack.calls.index("set:ready")
    assert live._store.items() == {"variable:dev:ready": "demo-two"} and state["demo"]["scenario"] == "demo-two"


def test_prepare_fails_plainly_when_cleanup_fails(live, stack):
    stack.failing_cleaners.add("variable")
    stack.seeded.add("dev")        # the stack still has its data, so this is no reset
    live._store.add({"variable:dev:old"}, "demo-two")
    demo = run(live, live.prepare, "demo-one")["demo"]
    assert demo["status"] == "failed" and "A setting changed in development" in demo["message"]


def test_baseline_failure_is_reported(live, stack, monkeypatch):
    del base.ACTIONS["dbt.run"]
    base.action("dbt.run", base.ACTIONS["fake.fail"].params)(lambda ctx, p: (_ for _ in ()).throw(
        base.StepFailed("The build broke.")))
    demo = run(live, live.prepare, "demo-one")
    assert demo["baseline"]["status"] == "failed" and "The build broke." in demo["baseline"]["detail"]
    assert demo["demo"]["status"] == "failed"


def test_background_baseline_job_and_feeder_start(live, stack):
    live.start()
    live.wait_idle(10)
    assert live.state()["baseline"]["status"] == "ready" and live.state()["feeder"]["running"]
    assert live._feeder.paused_check() is False
    live.stop()


def test_feeder_toggle_only_in_live_mode(live, make_engine):
    live.start()
    live.wait_idle(10)
    live.set_feeder(False)
    assert live.state()["feeder"]["running"] is False
    with pytest.raises(Conflict, match="only runs in live mode"):
        make_engine(mode="replay").set_feeder(True)
    live.stop()


def test_live_is_unavailable_without_secrets(make_engine):
    eng = make_engine(s2_root_pwd="", mode="live")
    assert eng.state()["mode"] == "replay" and "Live mode needs" in eng.state()["live_unavailable"]
    with pytest.raises(Conflict, match="Live mode needs"):
        eng.set_mode("live")


def test_mode_switch_requires_idle(live, stack):
    live.catalog.scenarios["demo-one"].demo.steps[0].do[0] = {"action": "fake.block"}
    open_demo(live)
    live.run_step("demo-one", "first", {})
    with pytest.raises(Conflict):
        live.set_mode("replay")
    stack.gate.set()
    live.wait_idle(5)
    live.set_mode("replay")
    assert live.state()["mode"] == "replay" and live.state()["demo"] is None


@pytest.fixture
def fast_replay(monkeypatch):
    monkeypatch.setattr(engine_module, "replay", lambda frames, apply, cancel: replay(frames, apply, cancel, min_seconds=0.05))


def test_replay_plays_a_recording_without_touching_the_stack(live, make_engine, stack, fast_replay):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    stack.calls.clear()
    eng = make_engine(mode="replay", s2_root_pwd="")
    state = run(eng, eng.prepare, "demo-one")
    assert state["demo"]["status"] == "ready" and "Replay mode" in state["notice"]["text"] and state["job"] is None
    state = run(eng, eng.run_step, "demo-one", "first", {})
    step = step_of(state, "first")
    assert step["status"] == "done" and step["outcome"] == "It worked." and step["evidence"][0]["data"]["rows"]
    assert stack.calls == []
    missing = step_of(run(eng, eng.run_step, "demo-one", "second", {}), "second")
    assert missing["status"] == "failed" and missing["summary"] == "This step has not been recorded yet."
    assert "nothing" in run(eng, eng.cleanup)["notice"]["text"]


def test_broker_coalesces_and_closes():
    broker = Broker()
    sub = broker.subscribe()
    assert sub.wait(1) == "change" and sub.wait(0.05) == "timeout"
    broker.notify(), broker.notify(), broker.notify()
    assert sub.wait(1) == "change" and sub.wait(0.05) == "timeout"
    started = time.monotonic()
    for _ in range(4):
        broker.notify()
        sub.wait(1)
    assert time.monotonic() - started >= 0.7                            # at most 4 pushes per second
    broker.close()
    assert sub.wait(1) == "closed"


@pytest.mark.parametrize("resource, label", [
    ("variable:dev:dbt_env_vars", "A setting changed in development"), ("landing:dev", "Demo data delivered to development"),
    ("git", "Demo copies of the code in GitLab"), ("version:prd", "A different version running in production"),
    ("models:dev", "Tables rebuilt in development"), ("weird:x", "Something changed by a demo")])
def test_leftover_labels(resource, label):
    assert leftover_label(resource) == label
