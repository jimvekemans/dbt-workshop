"""The quiz: a question for the room on a scenario or on a stage (`quiz:` in a scenario file)."""
from __future__ import annotations

import copy
from typing import Any

import pytest
from fastapi.testclient import TestClient

from studio.main import create_app
from studio.templating import TemplateContext
from tests.conftest import FakeFeeder, TODAY
from tests.test_flows_and_roadmap import flow, loads, made, problems

pytestmark = pytest.mark.usefixtures("fake_registry")


def quiz(**over: Any) -> dict[str, Any]:
    return {"situation": "A feed arrives broken at night and the load stops on it.",
            "question": "What do you do?",
            "options": [{"label": "Wait for the sender", "impact": "Every report is late.", "tone": "bad"},
                        {"label": "Leave that one table out tonight", "impact": "One report is late, the rest is on time.",
                         "tone": "good", "platform": True}],
            **over}


def test_a_scenario_and_a_stage_may_carry_a_quiz(settings, tmp_path):
    sc = flow(quiz=quiz(takeaway="One broken feed does not stop the night."))
    sc["demo"]["steps"][1]["quiz"] = quiz(minutes=3)
    catalog = loads(settings, tmp_path, sc)

    entry = catalog.groups_payload(lambda s: False)[0]["scenarios"][0]
    assert entry["quiz"]["question"] == "What do you do?"
    assert entry["quiz"]["minutes"] == 2 and entry["quiz"]["takeaway"].startswith("One broken feed")
    assert [o["platform"] for o in entry["quiz"]["options"]] == [False, True]
    assert [st["quiz"] is not None for st in entry["steps"]] == [False, True, False, False, False, False]
    assert entry["steps"][1]["quiz"]["minutes"] == 3

    payload = catalog.scenario_payload(sc["id"], TemplateContext("dev", {}, lambda: TODAY))
    assert payload["quiz"]["options"][1]["tone"] == "good"
    assert "quiz" not in payload["demo"]["steps"][0]
    assert payload["demo"]["steps"][1]["quiz"]["situation"].startswith("A feed arrives")


def test_a_scenario_and_a_stage_may_say_what_really_happened(settings, tmp_path):
    sc = flow(reality="In one release 15 of the 20 changes were taken out again.")
    sc["demo"]["steps"][1]["reality"] = "A rule with a typo caught nothing for 97 days."
    catalog = loads(settings, tmp_path, sc)

    entry = catalog.groups_payload(lambda s: False)[0]["scenarios"][0]
    assert entry["reality"].startswith("In one release")
    assert [st["reality"] for st in entry["steps"]][:3] == [None, "A rule with a typo caught nothing for 97 days.", None]

    payload = catalog.scenario_payload(sc["id"], TemplateContext("dev", {}, lambda: TODAY))
    assert payload["reality"].startswith("In one release")
    assert "reality" not in payload["demo"]["steps"][0]
    assert payload["demo"]["steps"][1]["reality"].startswith("A rule with a typo")


def test_without_it_the_payloads_say_none_and_a_long_or_empty_one_is_refused(settings, tmp_path):
    catalog = loads(settings, tmp_path, made())
    assert catalog.groups_payload(lambda s: False)[0]["scenarios"][0]["reality"] is None
    assert catalog.scenario_payload("demo-one", TemplateContext("dev", {}, lambda: TODAY))["reality"] is None
    assert any("reality" in p for p in problems(settings, tmp_path, made(reality="x" * 261)))
    assert any("reality" in p for p in problems(settings, tmp_path, made(reality="")))
    assert any("[[nowhere]]" in p for p in problems(settings, tmp_path, made(reality="A [[nowhere]] happened.")))


def test_without_a_quiz_the_payloads_say_none(settings, tmp_path):
    catalog = loads(settings, tmp_path, made())
    entry = catalog.groups_payload(lambda s: False)[0]["scenarios"][0]
    assert entry["quiz"] is None and all(st["quiz"] is None for st in entry["steps"])
    assert catalog.scenario_payload("demo-one", TemplateContext("dev", {}, lambda: TODAY))["quiz"] is None


def _options(*platform: bool) -> list[dict[str, Any]]:
    return [{"label": f"Choice {n}", "impact": "Something happens.", "tone": "warn", "platform": p}
            for n, p in enumerate(platform)]


@pytest.mark.parametrize("over, expected", [
    ({"options": _options(False, False)}, "marks exactly one option with 'platform: true'"),
    ({"options": _options(True, True)}, "this one marks 2"),
    ({"options": _options(True)}, "at least 2"),
    ({"options": _options(True, False, False, False)}, "at most 3"),
    ({"options": [{**o, "label": "Same"} for o in _options(True, False)]}, "need different labels"),
    ({"minutes": 6}, "less than or equal to 5"),
    ({"question": "x" * 91}, "at most 90"),
    ({"hint": "nope"}, "hint"),
])
def test_a_quiz_that_breaks_a_rule_is_refused(settings, tmp_path, over, expected):
    found = problems(settings, tmp_path, made(quiz=quiz(**over)))
    assert found and any(expected in p for p in found), found


def test_a_quiz_may_only_use_glossary_terms_that_exist(settings, tmp_path):
    found = problems(settings, tmp_path, made(quiz=quiz(situation="The [[nonsense]] is broken.")))
    assert any("[[nonsense]]" in p for p in found), found


def test_the_api_serves_the_quiz(settings, tmp_path):
    from dataclasses import replace
    from tests.conftest import write_catalog

    sc = made(quiz=quiz())
    folder = write_catalog(tmp_path / "api", [copy.deepcopy(sc)])
    app = create_app(replace(settings, scenarios_dir=folder), registry=False, feeder_factory=lambda: FakeFeeder(None))
    with TestClient(app, headers={"Host": "localhost", "X-Studio": "1"}) as client:
        entry = client.get("/api/catalog").json()["groups"][0]["scenarios"][0]
        assert entry["quiz"]["options"][1]["platform"] is True
        assert client.get(f"/api/scenarios/{sc['id']}").json()["quiz"]["question"] == "What do you do?"
