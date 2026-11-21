"""Flows (long scenarios), status labels and the closing overview (`_roadmap.yaml`)."""
from __future__ import annotations

import copy
from dataclasses import replace
from pathlib import Path
from typing import Any

import pytest
import yaml
from fastapi.testclient import TestClient

from studio.catalog import CatalogError, load_catalog
from studio.main import create_app
from studio.templating import TemplateContext
from tests.conftest import FakeFeeder, TODAY, scenario, write_catalog

pytestmark = pytest.mark.usefixtures("fake_registry")


def step(n: int, **extra: Any) -> dict[str, Any]:
    return {"id": f"stage_{n}", "title": f"Stage {n}", "button": "Go", "outcome": "It worked.",
            "do": [{"action": "fake.set", "key": f"k{n}"}], "show": [{"type": "table", "title": "Rows", "query": "select 1"}],
            **extra}


def made(sid: str = "demo-one", kind: str = "scenario", steps: int = 2, blocks: int = 1, minutes: int = 3,
         stage_fields: bool = False, **over: Any) -> dict[str, Any]:
    """The valid scenario with `steps` demo steps and `blocks` code blocks; `stage_fields` gives every step a feature and status."""
    sc = copy.deepcopy(scenario(sid))
    sc["kind"] = kind
    sc["minutes"] = minutes
    sc["demo"]["steps"] = [step(n, **({"feature": f"Thing {n}", "status": "today"} if stage_fields else {}))
                           for n in range(1, steps + 1)]
    sc["code"] = [copy.deepcopy(sc["code"][0]) for _ in range(blocks)]
    sc.update(over)
    return sc


def flow(**over: Any) -> dict[str, Any]:
    return made(kind="flow", steps=6, blocks=2, minutes=20, stage_fields=True, **over)


def problems(settings, tmp_path: Path, sc: dict[str, Any]) -> list[str]:
    folder = write_catalog(tmp_path / "bad", [])
    (folder / f"{sc['id']}.yaml").write_text(yaml.safe_dump(sc, sort_keys=False))
    (folder / "_groups.yaml").write_text(yaml.safe_dump({"groups": [{"id": "sources", "title": "S", "scenarios": [sc["id"]]}]}))
    try:
        load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    except CatalogError as e:
        return e.problems
    return []


def loads(settings, tmp_path: Path, *scenarios: dict[str, Any], roadmap: Any = None):
    folder = write_catalog(tmp_path / "ok", list(scenarios))
    if roadmap is not None:
        (folder / "_roadmap.yaml").write_text(yaml.safe_dump(roadmap, sort_keys=False))
    return load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)


# --- flows: limits ------------------------------------------------------------------------------------------------

@pytest.mark.parametrize("steps, blocks, minutes", [(3, 1, 1), (8, 8, 30), (6, 2, 20)])
def test_a_flow_inside_its_limits_is_accepted(settings, tmp_path, steps, blocks, minutes):
    sc = made(kind="flow", steps=steps, blocks=blocks, minutes=minutes, stage_fields=True)
    assert problems(settings, tmp_path, sc) == []


@pytest.mark.parametrize("over, expected", [
    ({"steps": 2}, "A flow has 3 to 8 demo steps, this one has 2"),
    ({"steps": 9}, "at most 8"),
    ({"blocks": 9}, "at most 8"),
    ({"minutes": 31}, "less than or equal to 30"),
])
def test_a_flow_outside_its_limits_is_refused(settings, tmp_path, over, expected):
    sc = made(kind="flow", stage_fields=True, **{"steps": 6, "blocks": 2, "minutes": 20, **over})
    found = problems(settings, tmp_path, sc)
    assert found and any(expected in p for p in found), found


@pytest.mark.parametrize("over, expected", [
    ({"steps": 5}, "A scenario has 2 to 4 demo steps, this one has 5. Make it a flow (kind: flow) for more."),
    ({"steps": 1}, "at least 2"),
    ({"blocks": 4}, "A scenario has 1 to 3 code blocks, this one has 4."),
    ({"minutes": 11}, "A scenario takes at most 10 minutes, this one says 11."),
    ({"minutes": 31}, "less than or equal to 30"),
])
def test_an_ordinary_scenario_keeps_its_limits(settings, tmp_path, over, expected):
    found = problems(settings, tmp_path, made(**over))
    assert any(expected in p for p in found), found


def test_the_old_limits_still_hold_for_a_file_without_a_kind(settings, tmp_path):
    sc = made(steps=4, blocks=3, minutes=10)
    del sc["kind"]
    assert problems(settings, tmp_path, sc) == []
    assert loads(settings, tmp_path, scenario("demo-one")).scenarios["demo-one"].kind == "scenario"


def test_an_unknown_kind_is_refused(settings, tmp_path):
    assert problems(settings, tmp_path, made(kind="saga"))


# --- status labels ------------------------------------------------------------------------------------------------

def test_step_feature_and_status_come_together(settings, tmp_path):
    only_feature = made(steps=2)
    only_feature["demo"]["steps"][0]["feature"] = "Release numbers"
    only_status = made(steps=2)
    only_status["demo"]["steps"][0]["status"] = "next"
    for sc in (only_feature, only_status):
        assert any("'feature' and 'status' come together" in p and "stage_1" in p for p in problems(settings, tmp_path, sc))
    both = made(steps=2)
    both["demo"]["steps"][0].update(feature="Release numbers", status="next")
    assert problems(settings, tmp_path, both) == []


@pytest.mark.parametrize("field, value", [("feature", "x" * 51), ("feature", ""), ("status", "soon")])
def test_step_feature_and_status_values_are_checked(settings, tmp_path, field, value):
    sc = made(steps=2)
    sc["demo"]["steps"][0].update({"feature": "Release numbers", "status": "today", field: value})
    assert problems(settings, tmp_path, sc)


def test_a_feature_of_exactly_fifty_characters_is_fine(settings, tmp_path):
    sc = made(steps=2)
    sc["demo"]["steps"][0].update(feature="x" * 50, status="today")
    assert problems(settings, tmp_path, sc) == []


def test_scenario_feature_and_status_come_together(settings, tmp_path):
    for over in ({"feature": "Late feeds"}, {"status": "today"}):
        assert any("the scenario: 'feature' and 'status' come together" in p
                   for p in problems(settings, tmp_path, made(**over)))
    assert problems(settings, tmp_path, made(feature="Late feeds", status="next")) == []
    assert problems(settings, tmp_path, made(feature="x" * 51, status="next"))


def test_code_block_status(settings, tmp_path):
    sc = made()
    sc["code"][0]["status"] = "next"
    catalog = loads(settings, tmp_path, sc)
    assert catalog.blocks["demo-one"][0]["status"] == "next"
    sc["code"][0]["status"] = "maybe"
    assert problems(settings, tmp_path, sc)
    assert "status" not in loads(settings, tmp_path, made()).blocks["demo-one"][0]    # absent stays absent


def test_in_a_flow_only_the_last_stage_may_leave_out_feature_and_status(settings, tmp_path):
    sc = flow()
    del sc["demo"]["steps"][-1]["feature"], sc["demo"]["steps"][-1]["status"]
    assert problems(settings, tmp_path, sc) == []                      # the "everything switched on" stage
    middle = flow()
    del middle["demo"]["steps"][2]["feature"], middle["demo"]["steps"][2]["status"]
    found = problems(settings, tmp_path, middle)
    assert any("every stage needs a 'feature' and a 'status', except the last one" in p and "stage_3" in p for p in found)
    first = flow()
    del first["demo"]["steps"][0]["feature"], first["demo"]["steps"][0]["status"]
    assert any("stage_1" in p for p in problems(settings, tmp_path, first))


def test_an_ordinary_scenario_needs_no_stage_fields(settings, tmp_path):
    assert problems(settings, tmp_path, made(steps=3)) == []


# --- payloads -----------------------------------------------------------------------------------------------------

def test_the_payloads_carry_kind_feature_status_and_stages(settings, tmp_path):
    sc = flow(feature="Releases", status="today")
    sc["code"][0]["status"] = "next"
    del sc["demo"]["steps"][-1]["feature"], sc["demo"]["steps"][-1]["status"]
    sc["demo"]["steps"][1]["status"] = "next"
    catalog = loads(settings, tmp_path, sc, made("demo-two"))
    one, two = catalog.groups_payload(lambda s: True)[0]["scenarios"]
    assert (one["kind"], one["feature"], one["status"]) == ("flow", "Releases", "today")
    assert [(x["feature"], x["status"]) for x in one["steps"]] == [
        ("Thing 1", "today"), ("Thing 2", "next"), ("Thing 3", "today"), ("Thing 4", "today"), ("Thing 5", "today"),
        (None, None)]
    assert (two["kind"], two["feature"], two["status"]) == ("scenario", None, None)
    payload = catalog.scenario_payload("demo-one", TemplateContext("dev", {}, lambda: TODAY))
    assert payload["kind"] == "flow" and (payload["feature"], payload["status"]) == ("Releases", "today")
    assert payload["demo"]["steps"][1]["status"] == "next" and "feature" not in payload["demo"]["steps"][-1]
    assert payload["code"][0]["status"] == "next"


def test_existing_scenarios_keep_their_old_payload_values(settings):
    catalog = load_catalog(settings, lambda: TODAY)
    entry = catalog.groups_payload(lambda s: False)[0]["scenarios"][0]
    assert entry["kind"] == "scenario" and entry["feature"] is None and entry["status"] is None


# --- why: what goes wrong without a next step ---------------------------------------------------------------------

def test_why_goes_with_feature_and_status_on_a_step(settings, tmp_path):
    lonely = made(steps=2)
    lonely["demo"]["steps"][0]["why"] = "Nobody sees a late feed."
    found = problems(settings, tmp_path, lonely)
    assert any("'why' only goes with 'feature' and 'status'" in p and "stage_1" in p for p in found), found
    for status in ("today", "next"):                     # optional for both statuses
        ok = made(steps=2)
        ok["demo"]["steps"][0].update(feature="Late feeds", status=status, why="Nobody sees a late feed.")
        assert problems(settings, tmp_path, ok) == []


def test_why_goes_with_feature_and_status_on_a_scenario(settings, tmp_path):
    found = problems(settings, tmp_path, made(why="Nobody sees a late feed."))
    assert any("the scenario: 'why' only goes with 'feature' and 'status'" in p for p in found), found
    assert problems(settings, tmp_path, made(feature="Late feeds", status="next", why="Nobody sees a late feed.")) == []
    assert problems(settings, tmp_path, made(feature="Late feeds", status="today", why="Still fine.")) == []


@pytest.mark.parametrize("why, fine", [("x" * 140, True), ("x" * 141, False), ("", False)])
def test_why_is_one_to_140_characters(settings, tmp_path, why, fine):
    sc = made(steps=2)
    sc["demo"]["steps"][0].update(feature="Late feeds", status="next", why=why)
    assert (problems(settings, tmp_path, sc) == []) is fine
    assert (problems(settings, tmp_path, made(feature="Late feeds", status="next", why=why)) == []) is fine


def test_why_is_served_next_to_feature_and_status(settings, tmp_path):
    sc = flow(feature="Releases", status="next", why="Every release is a manual job.")
    sc["demo"]["steps"][1]["why"] = "A broken table reaches the report."
    catalog = loads(settings, tmp_path, sc, made("demo-two"), roadmap={**ROADMAP, "extras": [
        {"area": "Access", "feature": "Own data per team", "status": "next", "why": "Teams see each other's data."}]})
    one, two = catalog.groups_payload(lambda s: True)[0]["scenarios"]
    assert one["why"] == "Every release is a manual job." and two["why"] is None
    assert [x["why"] for x in one["steps"]] == [None, "A broken table reaches the report.", None, None, None, None]
    assert catalog.roadmap_payload()["extras"][0]["why"] == "Teams see each other's data."


def test_files_without_why_keep_working(settings, tmp_path):
    catalog = loads(settings, tmp_path, flow(feature="Releases", status="today"), roadmap=ROADMAP)
    assert catalog.scenarios["demo-one"].why is None
    assert all(e.why is None for e in catalog.roadmap.extras)


# --- the roadmap file ---------------------------------------------------------------------------------------------

ROADMAP = {"title": "Where we stand, and the road ahead", "intro": "What works, and what comes next.",
           "closing": "Thank you.",
           "extras": [{"area": "Access", "feature": "Each team sees its own data", "status": "next", "note": "Planned."},
                      {"area": "Access", "feature": "Separate keys per environment", "status": "today"}]}


def test_no_roadmap_file_means_no_roadmap(settings):
    catalog = load_catalog(settings, lambda: TODAY)
    assert catalog.roadmap is None and catalog.roadmap_payload() is None


def test_a_roadmap_is_loaded_and_served(settings, tmp_path):
    catalog = loads(settings, tmp_path, scenario("demo-one"), roadmap=ROADMAP)
    payload = catalog.roadmap_payload()
    assert payload["title"] == ROADMAP["title"] and payload["closing"] == "Thank you."
    assert payload["extras"][0] == {"area": "Access", "feature": "Each team sees its own data", "status": "next",
                                    "note": "Planned.", "why": None}
    assert payload["extras"][1]["note"] is None and payload["extras"][1]["why"] is None


def test_the_minimal_roadmap_is_valid(settings, tmp_path):
    minimal = {"title": "Where we stand", "intro": "One sentence.", "extras": []}
    payload = loads(settings, tmp_path, scenario("demo-one"), roadmap=minimal).roadmap_payload()
    assert payload["extras"] == [] and payload["closing"] is None


@pytest.mark.parametrize("change, expected", [
    ({"title": "x" * 61}, "title"),
    ({"title": ""}, "title"),
    ({"intro": "x" * 301}, "intro"),
    ({"closing": "x" * 301}, "closing"),
    ({"surprise": 1}, "surprise"),
    ({"extras": [{"area": "x" * 41, "feature": "F", "status": "today"}]}, "area"),
    ({"extras": [{"area": "A", "feature": "x" * 61, "status": "today"}]}, "feature"),
    ({"extras": [{"area": "A", "feature": "F", "status": "someday"}]}, "status"),
    ({"extras": [{"area": "A", "feature": "F", "status": "today", "note": "x" * 161}]}, "note"),
    ({"extras": [{"area": "A", "feature": "F", "status": "today", "colour": "red"}]}, "colour"),
    ({"extras": [{"area": "A", "feature": "F"}]}, "status"),
    ({"extras": [{"area": "A", "feature": "F", "status": "next", "why": "x" * 141}]}, "why"),
    ({"extras": [{"area": "A", "feature": "F", "status": "next", "why": ""}]}, "why"),
    ({"intro": "About [[unknown term]]."}, "glossary term [[unknown term]]"),
    ({"extras": [{"area": "A", "feature": "A [[mystery]]", "status": "today"}]}, "glossary term [[mystery]]"),
])
def test_roadmap_problems_are_reported_in_plain_sentences(settings, tmp_path, change, expected):
    folder = write_catalog(tmp_path / "bad", [scenario("demo-one")])
    (folder / "_roadmap.yaml").write_text(yaml.safe_dump({**ROADMAP, **change}, sort_keys=False))
    with pytest.raises(CatalogError) as e:
        load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    assert any(p.startswith("_roadmap.yaml:") and expected in p for p in e.value.problems), e.value.problems


def test_a_roadmap_can_use_a_known_glossary_term(settings, tmp_path):
    roadmap = {**ROADMAP, "intro": "Our [[robots]] work."}
    assert loads(settings, tmp_path, scenario("demo-one"), roadmap=roadmap).roadmap is not None


def test_a_roadmap_file_is_not_taken_for_a_scenario(settings, tmp_path):
    catalog = loads(settings, tmp_path, scenario("demo-one"), roadmap=ROADMAP)
    assert set(catalog.scenarios) == {"demo-one"}


def test_the_api_serves_the_roadmap_in_the_catalog(settings, tmp_path, stack):
    folder = write_catalog(tmp_path / "scn", [flow(), scenario("demo-two")])
    (folder / "_roadmap.yaml").write_text(yaml.safe_dump(ROADMAP, sort_keys=False))
    app = create_app(replace(settings, scenarios_dir=folder), registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost") as c:
        body = c.get("/api/catalog").json()
        assert body["roadmap"]["title"] == ROADMAP["title"] and len(body["roadmap"]["extras"]) == 2
        assert [s["kind"] for s in body["groups"][0]["scenarios"]] == ["flow", "scenario"]
    (folder / "_roadmap.yaml").unlink()
    app = create_app(replace(settings, scenarios_dir=folder), registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost") as c:
        assert c.get("/api/catalog").json()["roadmap"] is None


# --- the idea preview on the home page tiles ------------------------------------------------------------------------

def test_the_home_payload_carries_the_idea_headline_and_visual_without_caption(settings, tmp_path):
    flowing = made("demo-one")
    flowing["idea"]["visual"] = {"type": "flow", "caption": "Dropped.", "links": [["a", "b"], ["b", "c"]],
                                 "nodes": [{"id": "a", "label": "A", "state": "ok", "note": "First"},
                                           {"id": "b", "label": "B", "state": "fix"},
                                           {"id": "c", "label": "C", "state": "ok"}]}
    catalog = loads(settings, tmp_path, flowing, made("demo-two"))   # `made` gives the second one a timeline idea
    one, two = catalog.groups_payload(lambda s: True)[0]["scenarios"]
    assert one["idea"]["headline"] == "Read both formats"
    visual = one["idea"]["visual"]
    assert visual["type"] == "flow" and "caption" not in visual
    assert [n["id"] for n in visual["nodes"]] == ["a", "b", "c"] and [list(x) for x in visual["links"]] == [["a", "b"], ["b", "c"]]
    assert visual["nodes"][0]["note"] == "First" and "note" not in visual["nodes"][1]       # empty fields are left out
    assert set(one["idea"]) == {"headline", "visual"}                                      # no points, no story
    assert two["idea"]["visual"]["type"] == "timeline" and "caption" not in two["idea"]["visual"]
    assert len(two["idea"]["visual"]["days"]) >= 5
