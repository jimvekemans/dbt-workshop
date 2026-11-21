"""Baseline speed and reset, waiting for the baseline check, plain check names, scenario order."""
from __future__ import annotations

import threading
from dataclasses import replace

import pytest
import yaml
from fastapi.testclient import TestClient

from studio import evidence, names, reset
from studio.actions import clients
from studio.actions.base import Cancelled
from studio.catalog import CatalogError, load_catalog
from studio.engine import WAITING, Conflict
from studio.feeder.writer import Writer
from studio.main import create_app
from tests.conftest import FakeFeeder, TODAY, scenario, write_catalog
from tests.test_engine import live, make_engine, open_demo, run  # noqa: F401  (fixtures)
from test_actions_support import make_ctx

pytestmark = pytest.mark.usefixtures("fake_registry")


# --- plain check names -------------------------------------------------------------------------------------------

@pytest.mark.parametrize("raw, plain", [
    ("source_not_null_raw_olist_raw_olist_orders_order_id", "Bestellingenfeed: het bestelnummer is nooit leeg"),
    ("unique_dim_olist_customers_customer_id", "Klantentabel: het klantnummer komt maar één keer voor"),
    ("accepted_values_stg_olist_orders_order_status__a__b", "Bestellingen (opgekuist): de bestelstatus heeft alleen toegelaten waarden"),
    ("relationships_fct_olist_orders_customer_id__customer_id__ref_dim_olist_customers_",
     "Bestellingentabel: elk klantnummer hoort bij een bestaande rij"),
    ("source_freshness_raw_carrier_raw_carrier_tracking_events", "Pakketscanfeed: de data is recent genoeg"),
    ("not_null_some_new_thing_col_id", "Check op some new thing col id"),
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
    assert out["rows"][0]["cells"]["name"] == "Bestellingenfeed: het bestelnummer is nooit leeg"
    assert out["rows"][0]["details"] == raw and out["rows"][1]["details"] == "dim_olist_customers"
    assert out["rows"][1]["cells"]["name"] == "Klantentabel"          # a known model: plain name
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
    assert sorted(listed) == files and len(listed) == len(set(listed)) and len(files) >= 16
    assert groups[0]["scenarios"]
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
    assert live.state()["waiting"]["label"] == "Alles opkuisen"
    with pytest.raises(Conflict, match="al een andere aanvraag"):
        live.cleanup()
    slow_baseline.set()
    for _ in range(100):
        live.wait_idle(10)
        if live.state()["notice"] and live.state()["waiting"] is None:
            break
        threading.Event().wait(0.05)
    assert live.state()["notice"]["text"] == "Alles staat weer zoals het was."


def test_a_baseline_check_that_takes_too_long_ends_the_wait(live, slow_baseline, monkeypatch):
    monkeypatch.setattr("studio.engine.BASELINE_WAIT", 0.1)
    live.prepare("demo-one")
    for _ in range(100):
        if live.state()["waiting"] is None:
            break
        threading.Event().wait(0.05)
    state = live.state()
    assert state["demo"]["status"] == "failed" and "langer over dan normaal" in state["demo"]["message"]


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
    with pytest.raises(Conflict, match="Nu niet: \"Rebuilding\" is nog bezig"):
        live.reset_state()
    stack.gate.set()


class Landing:
    """Stands in for SingleStore's landing area: the schema `<env>_landing` first, its tables later."""

    def __init__(self, tables_total=6):
        self.schema, self.tables, self.total, self.queries = False, 0, tables_total, []

    def connect(self, env):
        db = self

        class Cursor:
            def __enter__(self): return self
            def __exit__(self, *a): return False

            def execute(self, sql, params=None):
                db.queries.append((sql, params))

            def fetchone(self):
                return (db.tables,)

        class Conn:
            def cursor(self): return Cursor()
            def close(self): pass

        if not db.schema:
            raise RuntimeError("Unknown database 'dev_landing'")
        return Conn()


@pytest.fixture
def landing(live, monkeypatch):
    monkeypatch.setattr("studio.engine.DATABASE_POLL", 0.02, raising=False)
    db = Landing()
    live._get_feeder().writer = Writer(db.connect)
    return db


def wait_for(condition, seconds=5):
    for _ in range(int(seconds / 0.02)):
        if condition():
            return True
        threading.Event().wait(0.02)
    return False


def test_the_baseline_check_waits_for_the_landing_tables_not_just_the_schema(live, landing):
    done = threading.Event()
    threading.Thread(target=lambda: (live._wait_for_database(threading.Event()), done.set()), daemon=True).start()
    landing.schema = True                                   # the schema is there, bootstrap has not made the tables
    assert not done.wait(0.3), "it went on with an empty database"
    assert live.state()["baseline"]["detail"] == "Wachten tot de database opgestart is."
    landing.tables = 6
    assert done.wait(5)
    sql, params = landing.queries[-1]
    assert "information_schema.tables" in sql and params[0] == "prd_landing" and "raw_olist_customers" in params


def test_a_partly_created_landing_area_is_not_ready(live, landing):
    landing.schema, landing.tables = True, 3
    assert not live._get_feeder().writer.landing_ready("dev")
    landing.tables = 6
    assert live._get_feeder().writer.landing_ready("dev")


def test_waiting_for_the_database_gives_up_after_the_limit(live, landing, monkeypatch):
    monkeypatch.setattr("studio.engine.DATABASE_WAIT", 0.1)
    landing.schema = True                                   # the tables never come
    live._wait_for_database(threading.Event())              # returns: the check that follows reports the problem


@pytest.fixture
def cold_start(live):
    """A baseline check that is still waiting for the database, as after a cold start. Set the event to let it go on."""
    started, opened = threading.Event(), threading.Event()

    def wait(cancel):
        started.set()
        while not opened.is_set():
            if cancel.wait(0.02):
                raise Cancelled()

    live._wait_for_database = wait
    live.ensure_baseline()
    assert started.wait(5) and live.state()["job"]["scenario"] is None
    return opened


def test_reset_stops_a_running_baseline_check_instead_of_refusing(live, cold_start):
    first = live._thread
    live.reset_state()                                      # no Conflict
    assert not first.is_alive() and live._thread is not first
    assert live.state()["job"]["scenario"] is None          # a new baseline check runs
    cold_start.set()                                        # bootstrap has finished: the new check goes through
    live.wait_idle(10)
    state = live.state()
    assert state["baseline"]["status"] == "ready" and state["job"] is None and state["notice"] is None


def test_reset_drops_a_request_that_waited_for_the_baseline_check(live, cold_start):
    live.prepare("demo-one")
    assert live.state()["waiting"]["text"] == WAITING
    live.reset_state()
    assert live.state()["waiting"] is None and live.state()["demo"] is None
    for thread in [t for t in threading.enumerate() if t.name == "wait-baseline"]:
        thread.join(5)
    assert live.state()["demo"] is None                    # the dropped request did not start its demo
    cold_start.set()
    live.wait_idle(10)
    assert live.state()["baseline"]["status"] == "ready"


def test_reset_is_still_refused_while_a_scenario_is_getting_ready(live, stack):
    live._start_job("prepare", "demo-one", None, "Opening the scenario", lambda cancel: stack.gate.wait(5))
    with pytest.raises(Conflict, match="Nu niet: \"Opening the scenario\" is nog bezig"):
        live.reset_state()
    assert live.state()["job"] is not None and not live._cancel.is_set()      # and it was left alone
    stack.gate.set()


def test_reset_is_still_refused_during_a_cleanup(live, stack):
    live._start_job("cleanup", None, None, "Alles opkuisen", lambda cancel: stack.gate.wait(5))
    with pytest.raises(Conflict, match="Alles opkuisen"):
        live.reset_state()
    assert not live._cancel.is_set()
    stack.gate.set()


def test_reset_command_can_wait_until_the_baseline_is_ready(monkeypatch, capsys):
    states = iter([("missing", "Not checked yet.", True), ("failed", "Version v1.0.0 has not been built yet.", True),
                   ("preparing", "Checking the demo stack.", True), ("ready", "Every environment ...", False)])
    monkeypatch.setattr(reset, "read_baseline", lambda port: next(states))
    monkeypatch.setattr(reset.time, "sleep", lambda s: None)
    assert reset.wait_until_ready(8085, 600) == 0              # a "failed" while it still retries does not end it
    assert "is klaar" in capsys.readouterr().out


def test_reset_command_wait_reports_a_final_failure_and_a_timeout(monkeypatch, capsys):
    monkeypatch.setattr(reset.time, "sleep", lambda s: None)
    monkeypatch.setattr(reset, "read_baseline", lambda port: ("failed", "Version v1.0.0 has not been built yet.", False))
    assert reset.wait_until_ready(8085, 600) == 1 and "has not been built yet" in capsys.readouterr().out
    monkeypatch.setattr(reset, "read_baseline", lambda port: ("preparing", "Checking the demo stack.", True))
    monkeypatch.setattr(reset.time, "monotonic", iter([0, 1, 700, 701]).__next__)
    assert reset.wait_until_ready(8085, 600) == 1 and "niet klaar binnen 600 seconden" in capsys.readouterr().out


def test_reset_command_with_wait_exits_with_the_outcome(settings, monkeypatch):
    monkeypatch.setattr(reset, "ask_app", lambda port: (202, "Forgotten."))
    monkeypatch.setattr(reset, "wait_until_ready", lambda port, wait: 1)
    assert reset.main(settings, wait=30) == 1 and reset.main(settings) == 0
    monkeypatch.setattr(reset, "ask_app", lambda port: (409, "Not now: it runs."))
    assert reset.main(settings, wait=30) == 1


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
