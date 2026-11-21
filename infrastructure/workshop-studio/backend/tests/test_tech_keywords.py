"""`tech`: the technical keywords of a scenario and the technical line of a step (for the presenter)."""
from __future__ import annotations

import pytest

from dataclasses import replace

from fastapi.testclient import TestClient

from studio.main import create_app
from studio.templating import TemplateContext
from tests.conftest import FakeFeeder, TODAY, write_catalog
from tests.test_flows_and_roadmap import flow, loads, made, problems

pytestmark = pytest.mark.usefixtures("fake_registry")


def test_files_without_tech_keep_working(settings, tmp_path):
    assert problems(settings, tmp_path, made(steps=3)) == []
    catalog = loads(settings, tmp_path, made(steps=3))
    entry = catalog.groups_payload(lambda s: True)[0]["scenarios"][0]
    assert entry["tech"] is None and all(st["tech"] is None for st in entry["steps"])
    payload = catalog.scenario_payload("demo-one", TemplateContext("dev", {}, lambda: TODAY))
    assert payload["tech"] is None and all("tech" not in st for st in payload["demo"]["steps"])


def test_tech_is_served_for_a_scenario_and_its_steps(settings, tmp_path):
    sc = flow(tech=["vdp_ci", "--empty"])
    sc["demo"]["steps"][0]["tech"] = "vdp_ci + svc_ci_dbt (--target ci)"
    catalog = loads(settings, tmp_path, sc, made("demo-two", tech=["dbt retry"]))
    one, two = catalog.groups_payload(lambda s: True)[0]["scenarios"]
    assert one["tech"] == ["vdp_ci", "--empty"] and two["tech"] == ["dbt retry"]
    assert one["steps"][0]["tech"] == "vdp_ci + svc_ci_dbt (--target ci)" and one["steps"][1]["tech"] is None
    payload = catalog.scenario_payload("demo-one", TemplateContext("dev", {}, lambda: TODAY))
    assert payload["tech"] == ["vdp_ci", "--empty"]
    assert payload["demo"]["steps"][0]["tech"] == "vdp_ci + svc_ci_dbt (--target ci)"
    assert "tech" not in payload["demo"]["steps"][1]


@pytest.mark.parametrize("keywords, fine", [
    (["a"], True),
    (["x" * 40], True),
    ([f"k{n}" for n in range(8)], True),
    ([], False),
    ([f"k{n}" for n in range(9)], False),
    (["x" * 41], False),
    ([""], False),
    (["  "], False),
])
def test_scenario_tech_limits(settings, tmp_path, keywords, fine):
    assert (problems(settings, tmp_path, made(tech=keywords)) == []) is fine


@pytest.mark.parametrize("line, fine", [("x" * 60, True), ("x", True), ("x" * 61, False), ("", False), ("   ", False)])
def test_step_tech_limits(settings, tmp_path, line, fine):
    sc = made(steps=2)
    sc["demo"]["steps"][0]["tech"] = line
    assert (problems(settings, tmp_path, sc) == []) is fine


def test_the_messages_are_plain_sentences(settings, tmp_path):
    too_many = problems(settings, tmp_path, made(tech=[f"k{n}" for n in range(9)]))
    assert any("'tech' lists 1 to 8 keywords, this one has 9" in p for p in too_many), too_many
    too_long = problems(settings, tmp_path, made(tech=["x" * 41]))
    assert any("a keyword takes at most 40" in p for p in too_long), too_long
    sc = made(steps=2)
    sc["demo"]["steps"][0]["tech"] = "x" * 61
    line = problems(settings, tmp_path, sc)
    assert any("a step's technical line takes at most 60" in p for p in line), line


def test_tech_does_not_need_feature_and_status(settings, tmp_path):
    sc = made(steps=2, tech=["dbt retry"])
    sc["demo"]["steps"][0]["tech"] = "dbt retry"
    assert problems(settings, tmp_path, sc) == []


def test_unknown_keys_stay_forbidden(settings, tmp_path):
    sc = made(steps=2)
    sc["demo"]["steps"][0]["technical"] = "x"
    assert problems(settings, tmp_path, sc)
    assert problems(settings, tmp_path, made(technical=["x"]))


def test_the_api_serves_tech(settings, tmp_path, stack):
    sc = made(tech=["dbt retry"])
    sc["demo"]["steps"][0]["tech"] = "dbt retry"
    folder = write_catalog(tmp_path / "scn", [sc])
    app = create_app(replace(settings, scenarios_dir=folder), registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost") as c:
        entry = c.get("/api/catalog").json()["groups"][0]["scenarios"][0]
        assert entry["tech"] == ["dbt retry"] and entry["steps"][0]["tech"] == "dbt retry"
        body = c.get("/api/scenarios/demo-one").json()
        assert body["tech"] == ["dbt retry"] and body["demo"]["steps"][0]["tech"] == "dbt retry"
