"""Tests for the asset features: URI helper, the Variables, the selection of refreshed tables from run_results.json, the
asset events, the notice text and mail, and the command of dbt_on_demand (pure Python, no Airflow needed).

Run from airflow/:  python -m pytest tests
"""
import json
import os
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from utils.dbt_asset_utils import (  # noqa: E402
    add_row_counts, collect_events, count_rows, emit_events_enabled, get_outlet_path, parse_asset_dependencies,
    parse_bool, plan_events, read_asset_dependencies, render_refresh_notice)
from utils.dbt_command_utils import NO_NODES_IS_AN_ERROR, build_on_demand_command  # noqa: E402
from utils.dbt_config import ON_DEMAND_ARTIFACTS_DIR, ON_DEMAND_TARGET_DIR  # noqa: E402
from utils.dbt_failure_report import send_text_mail  # noqa: E402
from utils.dbt_state_utils import extract_refreshed_tables, select_refreshed_tables  # noqa: E402

NO_NODES = "'" + NO_NODES_IS_AN_ERROR + "'"      # as the shell line has it: one quoted word

# ---------------------------------------------------------------------------
# URI helper and Variables
# ---------------------------------------------------------------------------


def test_outlet_path_has_one_form_for_both_call_styles():
    assert get_outlet_path("dev_im_dimensional", "mart_olist_management_kpi", environment="dev") == (
        "s2://workshop-dev/dev_im_dimensional/mart_olist_management_kpi")
    assert get_outlet_path("val_edw_dv_bdv/stg_olist_orders", environment="val") == (
        "s2://workshop-val/val_edw_dv_bdv/stg_olist_orders")
    assert get_outlet_path("a", "b").startswith("s2://workshop-")


@pytest.mark.parametrize("args", [(), ("a",), ("a/b/c",), ("a", ""), ("", "b"), ("a", "b", "c"), ("a b", "c"), ("a", "b;c")])
def test_outlet_path_refuses_anything_else(args):
    with pytest.raises(ValueError):
        get_outlet_path(*args)


def test_asset_dependencies_parse_a_json_list_or_nothing():
    uris = ["s2://workshop-dev/dev_landing/raw_olist_orders", "s2://workshop-dev/dev_landing/raw_carrier_events"]
    assert parse_asset_dependencies(json.dumps(uris)) == uris
    assert parse_asset_dependencies(uris[0] + ",\n" + uris[1] + "," + uris[0]) == uris     # separated, no duplicates
    assert parse_asset_dependencies("[]") == [] and parse_asset_dependencies("") == [] and parse_asset_dependencies(None) == []
    for bad in ('["not a uri"]', '["raw_orders"]', "[not json", '["airflow://x/y"]', '{"a": 1}'):
        with pytest.raises(ValueError):
            parse_asset_dependencies(bad)


def test_reading_the_dependencies_never_breaks_the_dag_file():
    good = '["s2://workshop-dev/dev_landing/raw_olist_orders"]'
    assert read_asset_dependencies(lambda key, default=None: good) == ["s2://workshop-dev/dev_landing/raw_olist_orders"]
    assert read_asset_dependencies(lambda key, default=None: default) == []                # unset -> default "[]"
    assert read_asset_dependencies(lambda key, default=None: "[oops") == []                # broken -> clock schedule

    def unreachable(key, default=None):
        raise RuntimeError("variable store down")
    assert read_asset_dependencies(unreachable) == []


def test_the_emit_switch_is_off_unless_clearly_on():
    for on in ("true", "True", " 1 ", "yes", "ON", True):
        assert parse_bool(on) is True
    for off in ("false", "0", "no", "off", "", None, "maybe", False):
        assert parse_bool(off) is False
    assert emit_events_enabled(lambda key, default=None: default) is False
    assert emit_events_enabled(lambda key, default=None: "true") is True

    def unreachable(key, default=None):
        raise RuntimeError("variable store down")
    assert emit_events_enabled(unreachable) is False


# ---------------------------------------------------------------------------
# Selection of the refreshed tables
# ---------------------------------------------------------------------------


def _result(unique_id, status="success", relation=None, rows=None, seconds=1.5):
    res = {"unique_id": unique_id, "status": status, "execution_time": seconds, "adapter_response": {}}
    if relation:
        res["relation_name"] = relation
    if rows is not None:
        res["adapter_response"] = {"rows_affected": rows}
    return res


RUN_RESULTS = {
    "metadata": {"invocation_id": "inv-1"},
    "results": [
        _result("model.dbt_swf.dim_olist_customers", relation="`dev_im_dimensional`.`dim_olist_customers`", rows=96),
        _result("model.dbt_swf.mart_olist_management_kpi", relation="`dev_im_dimensional`.`mart_olist_management_kpi`", rows=24),
        _result("model.dbt_swf.fct_olist_orders", status="error", relation="`dev_im_dimensional`.`fct_olist_orders`"),
        _result("model.dbt_swf.fct_olist_deliveries", status="skipped", relation="`dev_im_dimensional`.`fct_olist_deliveries`"),
        _result("test.dbt_swf.unique_dim_olist_customers.abc", status="pass"),
        _result("seed.dbt_swf.some_seed", relation="`dev_edw_dv_rdv`.`some_seed`"),
        _result("operation.dbt_swf.dbt_swf-on-run-end-0", status="success"),
    ],
}


def test_only_successfully_built_models_are_announced():
    tables = select_refreshed_tables(RUN_RESULTS)
    assert [(t["database"], t["table"]) for t in tables] == [
        ("dev_im_dimensional", "dim_olist_customers"), ("dev_im_dimensional", "mart_olist_management_kpi")]
    assert tables[1]["rows_affected"] == 24 and tables[1]["execution_time"] == 1.5 and tables[1]["status"] == "success"


def test_a_model_without_row_count_or_relation_is_handled():
    manifest = {"nodes": {"model.dbt_swf.a": {"database": "dev_edw_dv_bdv", "alias": "a"}}}
    results = {"results": [_result("model.dbt_swf.a"), _result("model.dbt_swf.nothing_known")]}
    tables = select_refreshed_tables(results, manifest)
    assert tables == [{"database": "dev_edw_dv_bdv", "table": "a", "unique_id": "model.dbt_swf.a", "status": "success",
                       "rows_affected": None, "execution_time": 1.5}]
    assert select_refreshed_tables({}) == [] and select_refreshed_tables({"results": None}) == []
    odd = {"results": [_result("model.x.y", relation='"db"."tbl"', rows=True)]}          # quotes stripped, bool is no count
    assert select_refreshed_tables(odd)[0]["table"] == "tbl" and select_refreshed_tables(odd)[0]["rows_affected"] is None


def test_a_model_that_failed_its_own_test_is_not_announced():
    manifest = {"nodes": {"test.dbt_swf.not_null_kpi_x.1": {"depends_on": {"nodes": ["model.dbt_swf.mart_olist_management_kpi"]}},
                          "test.dbt_swf.warn_only.2": {"depends_on": {"nodes": ["model.dbt_swf.dim_olist_customers"]}}}}
    results = {"results": RUN_RESULTS["results"] + [
        _result("test.dbt_swf.not_null_kpi_x.1", status="fail"), _result("test.dbt_swf.warn_only.2", status="warn")]}
    assert [t["table"] for t in select_refreshed_tables(results, manifest)] == ["dim_olist_customers"]
    assert len(select_refreshed_tables(results)) == 2           # no manifest: only the status of the model counts


def test_extract_reads_the_files_and_skips_what_is_missing_or_stale(tmp_path):
    run_results = tmp_path / "run_results.json"
    manifest = tmp_path / "manifest.json"
    with pytest.raises(FileNotFoundError):                       # missing -> the task is skipped, not failed
        extract_refreshed_tables(str(run_results), str(manifest))
    run_results.write_text(json.dumps(RUN_RESULTS))
    payload = extract_refreshed_tables(str(run_results), str(manifest))     # manifest missing: still works
    assert payload["invocation_id"] == "inv-1" and len(payload["tables"]) == 2
    manifest.write_text("{not json")
    assert len(extract_refreshed_tables(str(run_results), str(manifest))["tables"]) == 2
    with pytest.raises(FileNotFoundError):                       # left by an earlier run: nothing built in this one
        extract_refreshed_tables(str(run_results), None, not_before=time.time() + 60)
    assert extract_refreshed_tables(str(run_results), None, not_before=time.time() - 60)["tables"]
    os.utime(run_results, (1, 1))
    with pytest.raises(FileNotFoundError):
        extract_refreshed_tables(str(run_results), None, not_before=time.time() - 60)


# ---------------------------------------------------------------------------
# Events
# ---------------------------------------------------------------------------


def test_one_event_per_table_with_the_result_in_extra():
    payload = {"invocation_id": "inv-1", "tables": select_refreshed_tables(RUN_RESULTS)}
    plan = plan_events(payload, image_tag="v1.0.0", environment="dev")
    assert [uri for uri, _ in plan] == [
        "s2://workshop-dev/dev_im_dimensional/dim_olist_customers",
        "s2://workshop-dev/dev_im_dimensional/mart_olist_management_kpi"]
    assert plan[1][1] == {"status": "success", "rows": None, "rows_affected": 24, "execution_time": 1.5,
                          "invocation_id": "inv-1", "dbt_image_tag": "v1.0.0", "environment": "dev"}
    json.dumps(plan)                                              # an event's extra must be JSON
    assert plan_events({"tables": []}, "v1") == [] and plan_events(None, "v1") == []


class FakeCursor:
    def __init__(self, counts, fail=()):
        self.counts, self.fail, self.last = counts, fail, None

    def execute(self, sql):
        self.last = sql.split("FROM ")[1].replace("`", "")
        if self.last in self.fail:
            raise RuntimeError("table is gone")

    def fetchone(self):
        return (self.counts[self.last],)


class FakeConnection:
    def __init__(self, counts, fail=()):
        self.cursor_, self.closed = FakeCursor(counts, fail), False

    def cursor(self):
        return self.cursor_

    def close(self):
        self.closed = True


CREDENTIALS = {"SINGLESTORE_HOST": "db", "SINGLESTORE_USER": "svc", "SINGLESTORE_PASS": "pw"}


def test_rows_are_counted_for_the_tables_that_can_be_counted():
    conn = FakeConnection({"d.a": 5, "d.b": 7}, fail=("d.gone",))
    tables = [("d", "a"), ("d", "gone"), ("d", "b"), ("d", "bad name")]
    got = count_rows(tables, CREDENTIALS, connect=lambda **kw: conn)
    assert got == {("d", "a"): 5, ("d", "b"): 7} and conn.closed          # a failing table or name is only left out


def test_no_credentials_no_driver_or_no_connection_means_no_counts_and_no_error():
    assert count_rows([("d", "a")], {}) == {}
    assert count_rows([], CREDENTIALS) == {}

    def refuses(**kw):
        raise ConnectionError("refused")
    assert count_rows([("d", "a")], CREDENTIALS, connect=refuses) == {}
    assert count_rows([("d", "a")], CREDENTIALS) == {}                    # no MySQLdb on the host


def test_the_counts_land_in_the_payload_and_the_events():
    payload = {"invocation_id": "inv-1", "tables": [{"database": "d", "table": "a", "status": "success", "rows_affected": None},
                                                     {"database": "d", "table": "b", "status": "success", "rows_affected": None}]}
    counted = add_row_counts(payload, lambda tables: {("d", "a"): 11})
    assert [t["rows"] for t in counted["tables"]] == [11, None] and "rows" not in payload["tables"][0]
    plan = plan_events(counted, "v1", environment="dev")
    assert plan[0][1]["rows"] == 11 and plan[1][1]["rows"] is None
    assert add_row_counts(None, lambda tables: {}) == {"tables": []}


# ---------------------------------------------------------------------------
# The notice
# ---------------------------------------------------------------------------

WHEN = datetime(2026, 11, 21, 6, 3, 42, tzinfo=timezone.utc)
EVENT = {"timestamp": WHEN, "extra": {"rows_affected": 1234, "dbt_image_tag": "v1.0.1", "status": "success"}}


class _Event:                                                     # what the Airflow context hands over
    def __init__(self, timestamp, extra):
        self.timestamp, self.extra = timestamp, extra


def test_notice_says_what_was_refreshed_in_plain_words():
    subject, body = render_refresh_notice([EVENT], "dev", "dev_im_dimensional", "mart_olist_management_kpi")
    assert subject == "[development] Het managementrapport is ververst"
    assert body.splitlines()[0] == "Het managementrapport is ververst."
    for line in ("Wanneer: 21 november, 06:03 UTC", "Versie: v1.0.1", "Rijen in het rapport: 1.234", "Omgeving: development"):
        assert line in body.splitlines()
    assert "Technische naam" not in body and "dev_im_dimensional" not in body and "mart_olist_management_kpi" not in body
    assert "verversingen sinds" not in body and len(body.splitlines()) < 12


def test_notice_describes_only_the_newest_of_several_events():
    older = {"timestamp": WHEN - timedelta(hours=2), "extra": {"rows_affected": 5, "dbt_image_tag": "v1.0.0"}}
    _, body = render_refresh_notice([older, EVENT], "prd", "prd_im_dimensional", "t")
    assert "Versie: v1.0.1" in body and "Omgeving: productie" in body and "(2 verversingen sinds de vorige verwittiging" in body


def test_notice_copes_with_a_missing_row_count_and_with_a_hand_started_run():
    _, body = render_refresh_notice([{"timestamp": None, "extra": {}}], "val", "val_im_dimensional", "t")
    assert "Rijen in het rapport: niet gemeld" in body and "Versie: niet bekend" in body and "Wanneer: niet bekend" in body
    subject, body = render_refresh_notice([], "dev", "dev_im_dimensional", "t")
    assert "manueel gestart" in subject and "manueel gestart" in body
    assert "Technische naam" not in body and "dev_im_dimensional" not in body


def test_the_notice_prefers_the_counted_rows_over_the_ones_dbt_reports():
    event = {"timestamp": WHEN, "extra": {"rows": 1234, "rows_affected": 5, "dbt_image_tag": "v1"}}
    assert "Rijen in het rapport: 1.234" in render_refresh_notice([event], "dev", "dev_x", "t")[1]
    no_rows = {"timestamp": WHEN, "extra": {"rows": None, "rows_affected": 5}}
    assert "Rijen in het rapport: 5" in render_refresh_notice([no_rows], "dev", "dev_x", "t")[1]


def test_an_event_listed_under_the_asset_and_under_its_alias_counts_once():
    """Airflow lists an event under its asset and under every alias it came through (TriggeringAssetEventsAccessor)."""
    asset = SimpleNamespace(uri="s2://workshop-dev/dev_im_dimensional/t")
    event = SimpleNamespace(timestamp=WHEN, extra={"rows": 3}, asset=asset, source_run_id="run-1", source_task_id="emit")
    other = SimpleNamespace(timestamp=WHEN, extra={"rows": 4}, asset=SimpleNamespace(uri="s2://x/y/z"), source_run_id="run-1", source_task_id="emit")
    events = collect_events({"asset": [event], "alias-dbt_refreshed_tables": [event, other]})
    assert sorted(e["extra"]["rows"] for e in events) == [3, 4]
    assert "verversingen sinds" not in render_refresh_notice(collect_events({"asset": [event], "alias": [event]}), "dev", "dev_x", "t")[1]


def test_events_are_collected_from_the_airflow_context_oldest_first():
    newer, older = _Event(WHEN, {"rows_affected": 1}), _Event(WHEN - timedelta(days=1), {"rows_affected": 2})
    events = collect_events({"asset-a": [newer, older]})
    assert [e["extra"]["rows_affected"] for e in events] == [2, 1]
    assert collect_events({"asset-a": [{"timestamp": None, "extra": None}]}) == [{"timestamp": None, "extra": {}}]
    assert collect_events({}) == [] and collect_events(None) == []


class FakeSMTP:
    sent: list = []

    def __init__(self, host, port, timeout=None):
        self.host, self.port = host, port

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def sendmail(self, sender, recipients, message):
        FakeSMTP.sent.append((self.host, self.port, sender, recipients, message))


MAIL_ENV = {"DBT_REPORT_SMTP_HOST": "mailpit", "DBT_REPORT_SMTP_PORT": "1025",
            "DBT_REPORT_MAIL_FROM": "airflow-dev@workshop.local", "DBT_FAILURE_EMAIL_RECIPIENTS": "data-team@workshop.local"}


def test_the_notice_goes_out_one_plain_text_mail_through_the_failure_report_settings():
    FakeSMTP.sent.clear()
    assert send_text_mail("subj", "body text", env=MAIL_ENV, smtp_factory=FakeSMTP)
    host, port, sender, recipients, message = FakeSMTP.sent[0]
    assert (host, port, sender, recipients) == ("mailpit", 1025, "airflow-dev@workshop.local", ["data-team@workshop.local"])
    assert len(FakeSMTP.sent) == 1 and "text/plain" in message and "text/html" not in message and "Subject: subj" in message
    assert send_text_mail("subj", "x", env={}, smtp_factory=FakeSMTP) is False         # not configured: no exception

    def refuse(*args, **kwargs):
        raise OSError("connection refused")
    assert send_text_mail("subj", "x", env=MAIL_ENV, smtp_factory=refuse) is False


# ---------------------------------------------------------------------------
# dbt_on_demand command
# ---------------------------------------------------------------------------


def _line(cmd):
    assert cmd[:2] == ["bash", "-c"] and len(cmd) == 3
    return cmd[2]


def test_on_demand_builds_the_selection_on_the_run_target_in_its_own_folder():
    assert _line(build_on_demand_command("tag:on-demand")) == (
        f"dbt build --select tag:on-demand --warn-error-options {NO_NODES} --target run --target-path {ON_DEMAND_TARGET_DIR}")
    assert "/target " not in _line(build_on_demand_command("a")) + " "                 # never the daily load's folder


def test_on_demand_exclude_and_vars_only_when_given():
    line = _line(build_on_demand_command("tag:on-demand  tag:x", exclude="a b", full_refresh_models=["m1", "m2"]))
    assert line == ("dbt build --select tag:on-demand tag:x --exclude a b --vars '{full_refresh_models: [m1, m2]}' "
                    f"--warn-error-options {NO_NODES} --target run --target-path {ON_DEMAND_TARGET_DIR}")
    plain = _line(build_on_demand_command("a", exclude="", full_refresh_models=[]))
    assert "--exclude" not in plain and "--vars" not in plain
    assert "[m1, m2]" in _line(build_on_demand_command("a", full_refresh_models='["m1", "m2"]'))   # JSON string works too


def test_on_demand_fails_when_the_selection_matches_nothing():
    assert "NoNodesForSelectionCriteria" in NO_NODES_IS_AN_ERROR and "NoNodesSelected" in NO_NODES_IS_AN_ERROR
    assert f"--warn-error-options {NO_NODES}" in _line(build_on_demand_command("tag:typo"))
    assert "--warn-error-options" not in _line(build_on_demand_command("tag:typo", try_number=2))     # a retry reads the state


def test_on_demand_retries_from_its_own_state_folder():
    line = _line(build_on_demand_command("tag:on-demand", try_number=2))
    assert line == f"dbt retry --state {ON_DEMAND_ARTIFACTS_DIR} --target run --target-path {ON_DEMAND_TARGET_DIR}"
    assert "daily_load" not in line


@pytest.mark.parametrize("kwargs", [
    {"select": ""}, {"select": "   "}, {"select": None},
    {"select": "a; rm -rf /"}, {"select": "$(id)"}, {"select": "a|b"}, {"select": "a", "exclude": "b`id`"},
    {"select": "a", "full_refresh_models": ["m1; drop"]}, {"select": "a", "full_refresh_models": ["m 1'"]},
])
def test_on_demand_refuses_empty_selection_and_shell_syntax(kwargs):
    with pytest.raises(ValueError):
        build_on_demand_command(**kwargs)


def test_on_demand_globs_are_quoted_for_the_shell():
    assert "'path:models/marts/*'" in _line(build_on_demand_command("path:models/marts/*"))
