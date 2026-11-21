"""The quick check of a prepare, plain names of accounts and databases, and the unit of charts."""
from __future__ import annotations

import pytest

from studio import evidence, names
from studio.actions import clients
from studio.actions.sql import describe_refusal
from test_actions_support import make_ctx
from test_engine import live, make_engine, open_demo, run  # noqa: F401 - the engine fixtures

pytestmark = pytest.mark.usefixtures("fake_registry")


def ready(live, stack):
    """A stack that is ready and clean, with the full check done and the calls forgotten."""
    stack.seeded.update({"dev", "val", "prd"})
    open_demo(live)
    run(live, live.cleanup)
    stack.calls.clear()


def versions_read(stack):
    return [c for c in stack.calls if c.startswith("versions:")]


def test_second_prepare_is_quick_and_skips_the_version_read(live, stack):
    stack.seeded.update({"dev", "val", "prd"})
    open_demo(live)
    assert len(versions_read(stack)) == 3 and not any(c.startswith("quick:") for c in stack.calls)
    run(live, live.cleanup)
    stack.calls.clear()
    open_demo(live)
    assert sorted(c for c in stack.calls if c.startswith("quick:")) == ["quick:dev", "quick:prd", "quick:val"]
    assert not versions_read(stack) and not any(c.startswith(("seed", "deploy", "build")) for c in stack.calls)
    assert live.state()["baseline"]["status"] == "ready"


def test_quick_check_that_finds_something_off_repairs_with_the_full_check(live, stack):
    ready(live, stack)
    stack.runs["val"] = False            # the built tables of validation are gone
    open_demo(live)
    assert "build:val" in stack.calls and versions_read(stack) == ["versions:val"]
    assert "build:dev" not in stack.calls


def test_full_check_after_a_failed_job(live, stack):
    ready(live, stack)
    live._full_due = False
    live._set_demo("failed", "x")
    open_demo(live)
    assert len(versions_read(stack)) == 3


def test_full_check_when_the_last_one_is_old(live, stack):
    ready(live, stack)
    live._last_full -= 601
    open_demo(live)
    assert len(versions_read(stack)) == 3
    run(live, live.cleanup)
    stack.calls.clear()
    open_demo(live)
    assert not versions_read(stack)


def test_full_check_after_a_reset(live, stack):
    ready(live, stack)
    live.reset_state()
    live.wait_idle(10)
    assert len(versions_read(stack)) == 3


def test_cleaned_setting_is_verified_only_where_it_was_cleaned(live, stack):
    ready(live, stack)
    live._store.add({"variable:val:dbt_image_tag"}, "demo-two")
    open_demo(live)
    assert versions_read(stack) == ["versions:val"]


def test_cleaned_other_resource_stays_quick(live, stack):
    ready(live, stack)
    live._store.add({"feed:dev:orders"}, "demo-two")
    open_demo(live)
    assert not versions_read(stack)


# --- plain names ---------------------------------------------------------------------------------------------

@pytest.mark.parametrize("name,words", [
    ("svc_dev_dbt", "de robot van development"), ("svc_val_dbt", "de robot van validatie"),
    ("svc_prd_dbt", "de robot van productie"), ("svc_ci_dbt", "de robot van de testrun"),
    ("pu_dev1", "developer één"), ("pu_dev2", "developer twee"), ("pu_dev3", "developer drie"),
    ("pu_admin", "de beheerder"),
])
def test_account_words(name, words):
    assert names.account_words(name) == words


@pytest.mark.parametrize("name,words", [
    ("dev_landing", "de binnenkomende data van development"), ("val_edw_dv_rdv", "de werktabellen van validatie"),
    ("prd_edw_dv_bdv", "de werktabellen van productie"), ("prd_im_dimensional", "de rapporttabellen van productie"),
    ("val_vdp_mgmt", "het logboek van validatie"), ("vdp_ci", "de sandbox voor de testrun"),
])
def test_database_words(name, words):
    assert names.database_words(name) == words


def test_plain_words_and_pair():
    assert names.plain_words("pu_dev1 mag niets wijzigen in prd_im_dimensional") == \
        "Developer één mag niets wijzigen in de rapporttabellen van productie"
    assert names.plain_words("Nothing technical here.") == "Nothing technical here."
    assert names.plain_words("lees dev_landing.raw_olist_orders") == "lees de bestellingenfeed in de binnenkomende data van development"
    assert names.plain_pair("svc_ci_dbt is in orde", "more") == ("De robot van de testrun is in orde", "svc_ci_dbt is in orde\nmore")
    assert names.plain_pair("All good.", "d") == ("All good.", "d")


def test_refusal_sentence_is_plain():
    text = "DELETE command denied to user 'pu_dev1'@'h' for table 'prd_im_dimensional.t'"
    assert describe_refusal("pu_dev1", text) == \
        "De database weigerde: developer één mag de tabellen in productie niet wijzigen."


def test_table_cells_with_names_are_plain_and_keep_the_technical_text(tmp_path, monkeypatch):
    monkeypatch.setattr(clients, "read_rows", lambda s, q, limit=200, timeout=5, user="root":
                        (["who", "db"], [("pu_admin", "prd_vdp_mgmt")]))
    p = evidence.TableParams.model_validate({"query": "q", "highlight": {"column": "who", "equals": "pu_admin"}})
    row = evidence.table(make_ctx(tmp_path), p)["rows"][0]
    assert row["cells"] == {"who": "De beheerder", "db": "Het logboek van productie"}
    assert row["state"] == "good" and row["details"] == "pu_admin; prd_vdp_mgmt"


def test_engine_shows_plain_sentences_and_keeps_technical_ones_in_details(live, stack):
    from studio.actions import base
    base.action("fake.names", base.ACTIONS["fake.fail"].params)(
        lambda ctx, p: base.Result("pu_dev2 veranderde dev_landing.", details="d"))
    sc = live.catalog.scenarios["demo-one"]
    entry = {"label": "x", "status": "running", "summary": "", "details": ""}
    live._run_one(sc, base.ACTIONS["fake.names"], base.ACTIONS["fake.names"].params(), entry, live._ctx("dev", live._cancel), lambda: None)
    assert entry["summary"] == "Developer twee veranderde de binnenkomende data van development."
    assert entry["details"] == "pu_dev2 veranderde dev_landing.\nd"


# --- units ---------------------------------------------------------------------------------------------------

@pytest.mark.parametrize("query,unit", [
    ("select d, count(*) from dev_landing.raw_carrier_tracking_events group by 1", "pakketscans"),
    ("select count(*) from prd_edw_dv_bdv.fct_olist_orders", "bestellingen"),
    ("select count(*) from raw_olist_order_payments", "betalingen"),
    ("select count(*) from raw_web_events", "websitebezoeken"),
    ("select 1", None),
])
def test_unit_is_guessed_from_the_tables(query, unit):
    assert evidence.unit_for(query) == unit


def test_unit_of_the_scenario_wins_for_bars_and_number(tmp_path, monkeypatch):
    monkeypatch.setattr(clients, "read_rows", lambda s, q, limit=200, timeout=5, user="root": (["d", "v"], [["mon", 2]]))
    ctx = make_ctx(tmp_path)
    assert evidence.bars(ctx, evidence.BarsParams(query="select 1 from orders", unit="late scans"))["unit"] == "late scans"
    assert evidence.bars(ctx, evidence.BarsParams(query="select d, v from raw_olist_orders"))["unit"] == "bestellingen"
    assert evidence.bars(ctx, evidence.BarsParams(query="select 1"))["unit"] is None
    assert evidence.number(ctx, evidence.NumberParams(query="select 1", unit="rows"))["unit"] == "rows"


# --- the quick check's SQL -----------------------------------------------------------------------------------

class FakeConn:
    def __init__(self, answer=(1,), error=None):
        self.sql, self.answer, self.error, self.closed = [], answer, error, False

    def cursor(self):
        return self

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def execute(self, sql):
        self.sql.append(sql)
        if self.error:
            raise self.error

    def fetchone(self):
        return self.answer

    def close(self):
        self.closed = True


def test_writer_quick_ready_is_one_read_only_statement():
    import pymysql
    from studio.feeder.writer import ALL_TABLES, Writer
    conn = FakeConn()
    assert Writer(lambda env: conn).quick_ready("val") is True
    assert len(conn.sql) == 1 and conn.sql[0].startswith("SELECT") and conn.closed
    assert all(t in conn.sql[0] for t in ALL_TABLES) and "'val_im_dimensional'" in conn.sql[0] and "'val_edw_dv_bdv'" in conn.sql[0]
    assert "dbt_run_results" not in conn.sql[0]   # only Airflow runs write the logbook: it says nothing about "built"
    assert Writer(lambda env: FakeConn((0,))).quick_ready("val") is False
    assert Writer(lambda env: FakeConn(error=pymysql.err.ProgrammingError(1146, "gone"))).quick_ready("val") is False
