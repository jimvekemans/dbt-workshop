"""Baseline speed and reset, waiting for the baseline check, plain check names, scenario order."""
from __future__ import annotations

import threading
from dataclasses import replace

import pytest
import yaml
from fastapi.testclient import TestClient

from studio import evidence, names, reset
from studio.actions import clients
from studio.catalog import CatalogError, load_catalog
from studio.engine import WAITING, Conflict
from studio.main import create_app
from tests.conftest import FakeFeeder, TODAY, scenario, write_catalog
from tests.test_engine import live, make_engine, open_demo, run  # noqa: F401  (fixtures)
from test_actions_support import make_ctx

pytestmark = pytest.mark.usefixtures("fake_registry")


# --- plain check names -------------------------------------------------------------------------------------------

@pytest.mark.parametrize("raw, plain", [
    ("source_not_null_raw_olist_raw_olist_orders_order_id", "Orders feed: the order number is never empty"),
    ("unique_dim_olist_customers_customer_id", "Customers table: the customer number appears only once"),
    ("accepted_values_stg_olist_orders_order_status__a__b", "Orders (cleaned): the order status only has allowed values"),
    ("relationships_fct_olist_orders_customer_id__customer_id__ref_dim_olist_customers_",
     "Orders table: every customer number belongs to a known record"),
    ("source_freshness_raw_carrier_raw_carrier_tracking_events", "Parcel scans feed: the data is recent enough"),
    ("not_null_some_new_thing_col_id", "Check on some new thing col id"),
])
def test_check_names_in_plain_words(raw, plain):
    assert names.looks_like_check(raw) and names.check_title(raw) == plain


def test_other_names_are_only_made_readable():
    assert not names.looks_like_check("dim_olist_customers") and names.check_title("dim_olist_customers") == "Olist customers"


def test_table_evidence_shows_plain_names_and_keeps_the_technical_one(tmp_path, monkeypatch):
    raw = "source_not_null_raw_olist_raw_olist_orders_order_id"
    monkeypatch.setattr(clients, "read_rows", lambda s, q, limit=200, timeout=5, user="root":
                        (["name", "failures"], [[raw, 3], ["dim_olist_customers", 0]]))
    out = evidence.table(make_ctx(tmp_path), evidence.TableParams.model_validate({"query": "select 1"}))
    assert out["rows"][0]["cells"]["name"] == "Orders feed: the order number is never empty"
    assert out["rows"][0]["details"] == raw and out["rows"][1]["details"] == "dim_olist_customers"
    assert out["rows"][1]["cells"]["name"] == "Customers table"          # a known model: plain name
    assert out["rows"][1]["details"] == "dim_olist_customers"            # the technical name is kept


# --- scenario order ----------------------------------------------------------------------------------------------

def groups_of(folder, scenarios_by_group):
    (folder / "_groups.yaml").write_text(yaml.safe_dump({"groups": [
        {"id": g, "title": g.title(), "scenarios": ids} for g, ids in scenarios_by_group.items()]}))


def test_catalog_follows_the_list_of_the_group(settings, tmp_path):
    folder = write_catalog(tmp_path / "ordered", [scenario("demo-one"), scenario("demo-two")])
    groups_of(folder, {"sources": ["demo-two", "demo-one"]})
    catalog = load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    assert [s["id"] for s in catalog.groups_payload(lambda sc: False)[0]["scenarios"]] == ["demo-two", "demo-one"]


@pytest.mark.parametrize("lists, expected", [
    ({"sources": ["demo-one"]}, "scenario 'demo-two' is missing from the list of group 'sources'"),
    ({"sources": ["demo-one", "demo-two", "demo-one"]}, "scenario 'demo-one' is listed twice"),
    ({"sources": ["demo-one", "demo-two", "ghost"]}, "lists 'ghost', which has no scenario file"),
    ({"sources": ["demo-one"], "daily": ["demo-two"]}, "listed under 'daily' but its file says group 'sources'"),
])
def test_catalog_check_fails_on_a_bad_order(settings, tmp_path, lists, expected):
    folder = write_catalog(tmp_path / "bad-order", [scenario("demo-one"), scenario("demo-two")])
    groups_of(folder, lists)
    with pytest.raises(CatalogError) as e:
        load_catalog(replace(settings, scenarios_dir=folder), lambda: TODAY)
    assert any(expected in p for p in e.value.problems), e.value.problems


def test_the_real_groups_file_lists_every_scenario_once():
    from pathlib import Path
    folder = Path(__file__).parents[2] / "scenarios"
    if not folder.is_dir():
        pytest.skip("the scenario files are not mounted")
    groups = yaml.safe_load((folder / "_groups.yaml").read_text())
    listed = [i for g in groups for i in g["scenarios"]]
    files = sorted(p.stem for p in folder.glob("*.yaml") if not p.name.startswith("_"))
    assert sorted(listed) == files and len(listed) == len(set(listed)) == 16
    assert groups[0]["scenarios"][0] == "switchover-date"
    for g in groups:
        assert all(yaml.safe_load((folder / f"{i}.yaml").read_text())["group"] == g["id"] for i in g["scenarios"])


# --- the baseline check ------------------------------------------------------------------------------------------

def test_a_wiped_environment_gets_its_version_again_even_when_the_setting_is_still_there(live, stack):
    stack.tags.update(val="v1.0.0", prd="v1.0.0", dev="v1.0.0")
    stack.runs.update(dev=True, val=True, prd=True)
    stack.seeded.update({"dev", "prd"})                    # validation lost its data in a reset
    open_demo(live)
    assert [c for c in stack.calls if c.startswith(("seed", "deploy", "build"))] == ["seed:val:90", "deploy:val:v1.0.0"]


def test_an_intact_stack_changes_nothing(live, stack):
    stack.tags.update(val="v1.0.0", prd="v1.0.0", dev="v1.0.0")
    stack.runs.update(dev=True, val=True, prd=True)
    stack.seeded.update({"dev", "val", "prd"})
    open_demo(live)
    assert not [c for c in stack.calls if c.startswith(("seed", "deploy", "build"))]


def test_after_a_reset_the_saved_state_is_forgotten_by_itself(live, stack, settings):
    live._store.add({"variable:dev:old", "git"}, "demo-two")      # describes the world before the reset
    (settings.data_dir / "git_baseline.json").write_text("{}")
    stack.failing_cleaners.add("variable")                         # it could not be put back any more
    demo = open_demo(live)["demo"]
    assert demo["status"] == "ready" and set(live._store.items()) == {"variable:dev:ready"}   # only the new demo's
    assert not (settings.data_dir / "git_baseline.json").exists()


# --- waiting for the baseline check ------------------------------------------------------------------------------

@pytest.fixture
def slow_baseline(live):
    release, started = threading.Event(), threading.Event()
    feeder = live._get_feeder()
    original = feeder.seed

    def slow(env, days=90):
        started.set()
        release.wait(10)
        original(env, days)

    feeder.seed = slow
    live.ensure_baseline()
    assert started.wait(5)
    yield release
    release.set()


def test_prepare_waits_for_the_baseline_check_instead_of_refusing(live, slow_baseline):
    live.prepare("demo-one")
    state = live.state()
    assert state["waiting"]["text"] == WAITING and state["demo"]["status"] == "preparing"
    assert state["demo"]["message"] == WAITING
    slow_baseline.set()
    for _ in range(100):
        live.wait_idle(10)
        if live.state()["demo"]["status"] == "ready":
            break
        threading.Event().wait(0.05)
    state = live.state()
    assert state["demo"]["status"] == "ready" and state["waiting"] is None


def test_cleanup_waits_too_and_a_second_request_is_refused(live, slow_baseline):
    live._store.add({"variable:dev:old"}, "demo-two")
    live.seeded = None
    live.cleanup()
    assert live.state()["waiting"]["label"] == "Putting everything back"
    with pytest.raises(Conflict, match="already waiting"):
        live.cleanup()
    slow_baseline.set()
    for _ in range(100):
        live.wait_idle(10)
        if live.state()["notice"] and live.state()["waiting"] is None:
            break
        threading.Event().wait(0.05)
    assert live.state()["notice"]["text"] == "Everything is back to normal."


def test_a_baseline_check_that_takes_too_long_ends_the_wait(live, slow_baseline, monkeypatch):
    monkeypatch.setattr("studio.engine.BASELINE_WAIT", 0.1)
    live.prepare("demo-one")
    for _ in range(100):
        if live.state()["waiting"] is None:
            break
        threading.Event().wait(0.05)
    state = live.state()
    assert state["demo"]["status"] == "failed" and "taking longer" in state["demo"]["message"]


# --- the reset command ---------------------------------------------------------------------------------------------

def test_reset_forgets_everything_and_checks_the_baseline_again(live, stack, settings):
    open_demo(live)
    run(live, live.run_step, "demo-one", "first", {})
    assert live.state()["leftovers"]
    (settings.data_dir / "git_baseline.json").write_text("{}")
    stack.calls.clear()
    live.reset_state()
    live.wait_idle(10)
    state = live.state()
    assert state["leftovers"] == [] and state["demo"] is None and state["baseline"]["status"] == "ready"
    assert not (settings.data_dir / "git_baseline.json").exists()


def test_reset_is_refused_while_a_step_runs(live, stack):
    open_demo(live)
    live._start_job("step", "demo-one", "first", "Rebuilding", lambda cancel: stack.gate.wait(5))
    with pytest.raises(Conflict, match="Not now: \"Rebuilding\" is still running"):
        live.reset_state()
    stack.gate.set()


def test_reset_over_http(settings, stack):
    app = create_app(settings, registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost:8085") as client:
        app.state.engine.wait_idle(10)
        assert client.post("/api/reset", headers={"X-Studio": "1"}).status_code == 202
        assert client.post("/api/reset").status_code == 403           # the guard header is needed


def test_reset_command_prints_the_sentence_and_the_exit_code(settings, monkeypatch, capsys):
    monkeypatch.setattr(reset, "ask_app", lambda port: (409, "Not now: it runs."))
    assert reset.main(settings) == 1 and "Not now: it runs." in capsys.readouterr().out
    monkeypatch.setattr(reset, "ask_app", lambda port: (202, "Forgotten."))
    assert reset.main(settings) == 0


def test_reset_command_without_a_running_app_removes_the_files(settings, monkeypatch, capsys):
    settings.data_dir.mkdir(parents=True, exist_ok=True)
    for name in reset.SAVED:
        (settings.data_dir / name).write_text("{}")
    monkeypatch.setattr(reset, "ask_app", lambda port: (0, ""))
    assert reset.main(settings) == 0
    assert not any((settings.data_dir / n).exists() for n in reset.SAVED)
