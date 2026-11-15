"""Fakes for the real actions, cleaners and evidence providers, so the engine is tested on its own.

The `fake_registry` fixture is opt-in (modules use `pytestmark`), so tests of the real actions keep the real registry."""
from __future__ import annotations

import copy
import threading
from datetime import date
from pathlib import Path
from typing import Any

import pytest
import yaml
from pydantic import ConfigDict

from studio.actions import base
from studio.actions.base import Params, Result, StepFailed
from studio.config import Settings

FIXTURES = Path(__file__).parent / "fixtures"
TODAY = date(2026, 3, 10)


class Free(Params):
    """Fake actions accept anything so tests can shape their own parameters."""

    model_config = ConfigDict(extra="allow")


class FakeStack:
    """What the fake stack remembers, and the calls made on it, in order."""

    def __init__(self) -> None:
        self.calls: list[str] = []
        self.tags: dict[str, str | None] = {"dev": None, "val": None, "prd": None}
        self.runs: dict[str, bool] = {"dev": False, "val": False, "prd": False}
        self.seeded: set[str] = set()
        self.gate = threading.Event()          # released by tests to let `fake.block` finish
        self.failing_cleaners: set[str] = set()
        self.hold_cleaners: threading.Event | None = None   # when set, a cleaner waits until this event is released


class FakeFeeder:
    def __init__(self, stack: FakeStack) -> None:
        self.stack, self.running, self.paused_check = stack, False, None

    def is_seeded(self, env: str) -> bool:
        return env in self.stack.seeded

    def quick_ready(self, env: str) -> bool:
        self.stack.calls.append(f"quick:{env}")
        return env in self.stack.seeded and self.stack.runs[env]

    def seed(self, env: str, days: int = 90) -> None:
        self.stack.calls.append(f"seed:{env}:{days}")
        self.stack.seeded.add(env)

    def today(self, env: str) -> date:
        return TODAY

    def status(self) -> dict[str, Any]:
        return {"running": self.running, "simulated_day": {e: TODAY.isoformat() for e in ("dev", "val", "prd")},
                "last_delivery": None}

    def start_background(self, interval: float, should_pause: Any) -> None:
        self.running, self.paused_check = True, should_pause

    def stop(self) -> None:
        self.running = False


@pytest.fixture
def stack() -> FakeStack:
    return FakeStack()


@pytest.fixture
def fake_registry(stack: FakeStack):
    saved = copy.copy(base.ACTIONS), copy.copy(base.CLEANERS), copy.copy(base.EVIDENCE)
    base.ACTIONS.clear(), base.CLEANERS.clear(), base.EVIDENCE.clear()

    def act(name: str):
        return base.action(name, Free)

    @act("fake.set")
    def _set(ctx, p):
        stack.calls.append(f"set:{p.key}")
        return Result(f"Set {p.key}.", touched={f"variable:{ctx.env}:{p.key}"}, data={"key": p.key})

    @act("fake.land")
    def _land(ctx, p):
        stack.calls.append("land")
        return Result("Data delivered.", touched={f"landing:{ctx.env}", f"models:{ctx.env}"})

    @act("fake.fail")
    def _fail(ctx, p):
        raise StepFailed("The load did not finish.", "details")

    @act("fake.boom")
    def _boom(ctx, p):
        raise RuntimeError("kaput")

    @act("fake.block")
    def _block(ctx, p):
        while not stack.gate.wait(0.02):
            ctx.check_cancel()
        return Result("Done waiting.")

    @act("fake.sleep")
    def _sleep(ctx, p):
        ctx.sleep(30)
        return Result("never")

    @act("deploy.set_version")
    def _deploy(ctx, p):
        stack.calls.append(f"deploy:{ctx.env}:{p.tag}")
        stack.tags[ctx.env] = p.tag
        return Result("Version set.")

    @act("dbt.run")
    def _dbt(ctx, p):
        stack.calls.append(f"build:{ctx.env}")
        stack.runs[ctx.env] = True
        return Result("Built.")

    for name in ("sql.exec", "gitlab.branch", "gitlab.commit", "feed.condition", "feed.deliver"):
        act(name)(lambda ctx, p: Result("ok"))

    for prefix in ("variable", "landing", "models", "feed", "git"):
        def make(prefix: str):
            @base.cleaner(prefix)
            def clean(ctx, resource):
                stack.calls.append(f"clean:{resource}")
                if stack.hold_cleaners is not None:
                    stack.hold_cleaners.wait(10)
                if prefix in stack.failing_cleaners:
                    raise StepFailed("It would not go back.")
                return Result(f"Put back {resource}.")
        make(prefix)

    @base.evidence("table", Free)
    def _table(ctx, p):
        return {"rows": [[ctx.env, getattr(p, "query", "")]]}

    @base.evidence("number", Free)
    def _number(ctx, p):
        raise RuntimeError("provider down")

    @base.evidence("result", Free)
    def _result(ctx, p):
        return {"summary": ctx.last_result.summary if ctx.last_result else ""}

    @base.evidence("versions", Free)
    def _versions(ctx, p):
        stack.calls.append(f"versions:{getattr(p, 'only', '')}")
        return {"environments": [{"env": e, "tag": t} for e, t in stack.tags.items()]}

    @base.evidence("run_history", Free)
    def _history(ctx, p):
        return {"runs": [{"state": "good"}] if stack.runs[ctx.env] else []}

    yield
    base.ACTIONS.clear(), base.CLEANERS.clear(), base.EVIDENCE.clear()
    base.ACTIONS.update(saved[0]), base.CLEANERS.update(saved[1]), base.EVIDENCE.update(saved[2])


# --- scenario content --------------------------------------------------------------------------------------------

STORY = " ".join(["Maria opens the weekly report on Monday morning and finds that two days of deliveries are missing."] * 3)


def visual(kind: str = "flow") -> dict[str, Any]:
    if kind == "timeline":
        return {"type": "timeline", "days": [{"label": f"Day {i}", "state": "ok"} for i in range(5)],
                "marker": {"after": 2, "label": "Switch"}}
    nodes = [{"id": f"n{i}", "label": f"Step {i}", "state": "ok"} for i in range(3)]
    return {"type": "flow", "nodes": nodes, "links": [["n0", "n1"], ["n1", "n2"]]}


def scenario(sid: str = "demo-one", **over: Any) -> dict[str, Any]:
    data: dict[str, Any] = {
        "id": sid, "group": "sources", "title": "A demo about [[robots]]", "promise": "Nothing is lost.", "minutes": 3,
        "environment": "dev",
        "problem": {"headline": "Deliveries vanish", "story": STORY, "cost": "Customers wait.", "visual": visual()},
        "idea": {"headline": "Read both formats", "points": ["One.", "Two.", "Three."], "visual": visual("timeline")},
        "code": [{"title": "The model reads both formats", "file": "models/events.sql", "from": "old_format as (",
                  "to": "from {{ source('carrier', 'raw_events_v2') }}", "max_lines": 20,
                  "notes": [{"line": "where event_ts <", "says": "Old format before the cut-over."}]}],
        "demo": {
            "intro": "Pick a day.",
            "setup": [{"action": "fake.set", "key": "ready"}],
            "steps": [
                {"id": "first", "title": "First", "button": "Go", "outcome": "It worked.",
                 "do": [{"action": "fake.set", "key": "one"}],
                 "show": [{"type": "table", "title": "Rows", "query": "select 1"}]},
                {"id": "second", "title": "Second", "button": "Go again", "outcome": "Fixed.",
                 "inputs": [{"id": "pick", "type": "choice", "label": "Pick", "default": "a",
                             "options": [{"value": "a", "label": "A"}, {"value": "b", "label": "B"}]},
                            {"id": "day", "type": "day", "label": "Day", "default": "{{ day(-1) }}",
                             "min": "{{ day(-7) }}", "max": "{{ day(0) }}"}],
                 "do": [{"action": "fake.set", "key": "two", "value": "{{ inputs.pick }}"}],
                 "show": [{"type": "table", "title": "Rows", "query": "select '{{ inputs.day }}'"},
                          {"type": "result", "title": "What happened"}]},
            ]},
    }
    data.update(over)
    return data


def write_catalog(directory: Path, scenarios: list[dict[str, Any]]) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    groups = {"groups": [{"id": "sources", "title": "When the data changes",
                          "scenarios": [sc["id"] for sc in scenarios if sc.get("group", "sources") == "sources"]}]}
    (directory / "_groups.yaml").write_text(yaml.safe_dump(groups))
    (directory / "_glossary.yaml").write_text(yaml.safe_dump({"terms": {"robots": "Programs that do chores."}}))
    for sc in scenarios:
        (directory / f"{sc['id']}.yaml").write_text(yaml.safe_dump(sc, sort_keys=False))
    return directory


@pytest.fixture
def settings(tmp_path: Path) -> Settings:
    """Live-capable settings (fake secrets) on throw-away directories."""
    return Settings(scenarios_dir=write_catalog(tmp_path / "scenarios", [scenario("demo-one"), scenario("demo-two")]),
                    recordings_dir=tmp_path / "recordings", data_dir=tmp_path / "data",
                    dbt_project_dir=FIXTURES / "dbt", static_dir=tmp_path / "nostatic",
                    s2_root_pwd="x", gitlab_pat="y", mode="live")
