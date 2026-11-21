"""Tests for utils/dbt_failure_report.py (pure Python, no Airflow needed).

Every fixture is REAL dbt 2.0.11 output. tests/fixtures/*.log are the dbt_build task logs of the dev Airflow
(2026-11-15, image dbt-vdp:v1.0.0), reduced to the text of the log events. NOT covered by a real capture: a failed
MODEL line (the parser assumes the shape of the test line, see test_failed_model_line_is_assumed_like_test_line).
"""
import html
import json
import os
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from utils.dbt_failure_report import (  # noqa: E402
    build_and_send, count_note, describe_failure, describe_test, load_run_results, merge_run_results, parse_log, render_html, render_subject,
    send_report, summary_line, tally)

from utils.dbt_failure_report import Finding  # noqa: E402

FIXTURES = Path(__file__).parent / "fixtures"
CTX = {"environment": "dev", "dag_id": "dbt_daily_load", "run_id": "scheduled__2026-11-15T00:00:00+00:00",
       "image_tag": "v1.0.0", "run_url": "http://localhost:8088/dags/dbt_daily_load/runs/x"}

# Source: Airflow task log dbt_daily_load / scheduled__2026-11-15T00:00:00+00:00 / dbt_build (not_null, 3 NULLs)
FAILED_TEST = (FIXTURES / "failed_test_with_detail.log").read_text()
# Source: dbt_daily_load / probe_failed_model3 / dbt_build (two tests failed because their source table was missing)
FAILED_TESTS_DB_ERROR = (FIXTURES / "failed_tests_db_not_found.log").read_text()
# Source: dbt_daily_load / studio__1794746799_ffc3d1 / dbt_build (a dbt_utils test that only warns)
WARNED_TEST = (FIXTURES / "warned_test.log").read_text()

# Source: dbt 2.0.11 `dbt build` with a model that refs a missing node (parse-time failure, run in this stack)
PARSE_ERROR = """\
   dbt-oss 2.0.11
   Loading profiles.yml
   Loading packages.yml
     Using 3 threads (target: run)

=================== Errors and Warnings ====================
[error] [DependencyNotFound (dbt1048)]: Ref 'does_not_exist' not found in project. Searched for 'dbt_swf.does_not_exist, does_not_exist'
  --> models/im_dimensional/broken_model.sql:1:15

==================== Execution Summary =====================
Finished 'build' with 1 error for target 'run' [162ms]
"""

# Source: dbt 2.0.11, logs/dbt.log of this stack (connection failure before any node ran)
CONNECTION_ERROR = """\
[error] [AuthenticationFailed (dbt1011)]: dbt was unable to connect to the database configured for the `singlestore` adapter.
The following error was returned:

Failed to connect to SingleStore: error communicating with database: No address associated with hostname

Check your database credentials and try again.
==================== Execution Summary =====================
Finished 'build' with 1 error for target 'run' [176ms]
"""

# Source: the exact lines quoted from dbt 2.0.11 in the task (also present in the fixtures above)
RESULT_LINES = """\
 Succeeded model dim_olist_customers (table) [30 of 49 in 0.64s]
 Succeeded model stg_olist_orders (incremental) [12 of 49 in 0.15s]
   Passed test  unique_stg_carrier_events_event_id [34 of 49 in 0.10s]
   Failed test  source_not_null_raw_olist_raw_olist_orders_order_id (models/edw_dv_bdv/sources_olist.yml:40:17) [2 of 49 in 0.00s]
   Warned test  dbt_utils_source_expression_is_true_raw_olist_raw_olist_orders_customer_id__regexp_0_9a_f_32_ (models/edw_dv_bdv/sources_olist.yml:47:17) [9 of 49 in 0.01s]
  Skipped model mart_olist_management_kpi (table) [46 of 49]
"""


def test_failed_test_with_detail_line():
    report = parse_log(FAILED_TEST)
    assert len(report.errors) == 1
    failed = report.errors[0]
    assert (failed.node, failed.resource_type) == ("source_not_null_raw_olist_raw_olist_orders_order_id", "test")
    assert failed.failures == 3 and failed.location == "models/edw_dv_bdv/sources_olist.yml:40:17"
    assert failed.duration == "0.01s" and failed.message == ""
    assert report.counts == {"pass": 42, "warn": 0, "error": 1, "skip": 6}      # dbt says 43: it counts the hook
    assert report.dbt_success == 43 and report.hooks == 1
    assert report.summary == "Summary: 50 total | 43 success | 1 error | 6 skipped"
    assert report.processed == "Processed: 1 hook | 28 models | 21 tests"


def test_skipped_nodes_are_listed_not_errors():
    report = parse_log(FAILED_TEST)
    assert {f.node for f in report.skipped if f.resource_type == "model"} == {
        "stg_olist_orders", "fct_olist_orders", "mart_web_conversion", "mart_olist_management_kpi"}
    assert all(f.node not in {e.node for e in report.errors} for f in report.skipped)
    assert report.skipped[0].materialization == "incremental" or report.skipped[0].resource_type == "test"


def test_error_block_names_the_failed_tests():
    report = parse_log(FAILED_TESTS_DB_ERROR)
    assert [f.node for f in report.errors] == ["source_not_null_raw_carrier_raw_carrier_tracking_events_event_id",
                                               "assert_carrier_events_delivered"]
    assert all("dev_landing.raw_carrier_tracking_events' doesn't exist" in f.message for f in report.errors)
    assert report.errors[1].location == "tests/carrier/assert_carrier_events_delivered.sql"
    assert report.counts == {"pass": 39, "warn": 0, "error": 2, "skip": 8}


def test_warned_test_has_no_detail_line():
    report = parse_log(WARNED_TEST)
    assert not report.errors
    assert [w.node for w in report.warnings] == [
        "dbt_utils_source_expression_is_true_raw_olist_raw_olist_orders_customer_id__regexp_0_9a_f_32_"]
    assert report.warnings[0].location == "models/edw_dv_bdv/sources_olist.yml:47:17"
    assert report.counts == {"pass": 48, "warn": 1, "error": 0, "skip": 0}


def test_parse_error_block_names_the_model():
    report = parse_log(PARSE_ERROR)
    assert [(f.node, f.resource_type) for f in report.errors] == [("broken_model", "model")]
    assert "DependencyNotFound (dbt1048)" in report.errors[0].message and "does_not_exist" in report.errors[0].message
    assert report.errors[0].location == "models/im_dimensional/broken_model.sql:1:15"
    assert report.counts["error"] == 1


def test_error_before_any_node_ran():
    report = parse_log(CONNECTION_ERROR)
    assert len(report.errors) == 1 and report.errors[0].node == ""
    assert "No address associated" in report.errors[0].message
    assert report.counts["error"] == 1
    assert render_subject(report, CTX) == "[development] Nachtelijke load mislukt: 1 fout"


def test_result_lines_airflow_prefix_json_and_ansi():
    wrapped = "\n".join(json.dumps({"timestamp": "2026-11-15T11:44:13Z", "level": "info", "event": line})
                        for line in RESULT_LINES.splitlines())
    report = parse_log("\x1b[1m" + wrapped)
    assert [f.node for f in report.errors] == ["source_not_null_raw_olist_raw_olist_orders_order_id"]
    assert len(report.warnings) == 1 and len(report.skipped) == 1
    assert report.counts == {"pass": 3, "warn": 1, "error": 1, "skip": 1}
    text = "[2026-11-15T11:44:13.000+0000] {x.py:1} INFO - " + RESULT_LINES.splitlines()[0].strip()
    assert parse_log(text).counts["pass"] == 1


def test_failed_model_line_is_assumed_like_test_line():
    # INFERRED, not captured: dbt 2 prints a failed model like a failed test, with the materialization in brackets.
    report = parse_log("   Failed model dim_b (table) [5 of 49 in 0.1s]\n")
    assert [(f.node, f.resource_type, f.materialization, f.duration) for f in report.errors] == [
        ("dim_b", "model", "table", "0.1s")]
    assert render_subject(report, CTX) == "[development] Nachtelijke load mislukt: 1 tabel mislukt"
    assert "Model mislukt: dim_b" in render_html(report, CTX)


def test_readable_names():
    assert describe_test("source_not_null_raw_olist_raw_olist_orders_order_id") == (
        "order_id in raw_olist_orders mag nooit leeg zijn")
    assert describe_test("source_not_null_raw_olist_raw_olist_order_payments_order_id") == (
        "order_id in raw_olist_order_payments mag nooit leeg zijn")
    assert describe_test("unique_stg_carrier_events_event_id") == "event_id in stg_carrier_events moet uniek zijn"
    assert describe_test("accepted_values_stg_carrier_events_event_type__picked_up__in_transit__delivered") == (
        "event_type in stg_carrier_events mag enkel de toegelaten waarden bevatten (picked up, in transit, delivered)")
    assert describe_test("dbt_utils_unique_combination_of_columns_fct_olist_orders_order_hkey") == (
        "de combinatie van order_hkey in fct_olist_orders moet uniek zijn")
    assert describe_test("dbt_utils_source_unique_combination_of_columns_raw_olist_raw_olist_order_payments_"
                         "order_id__payment_sequential") == (
        "de combinatie van order_id, payment_sequential in raw_olist_order_payments moet uniek zijn")
    assert describe_test("dbt_utils_source_expression_is_true_raw_olist_raw_olist_orders_customer_id"
                         "__regexp_0_9a_f_32_") == (
        "customer_id in raw_olist_orders moet aan een regel voldoen (regexp_0_9a_f_32_)")
    assert describe_test("assert_carrier_events_delivered") is None      # singular test: raw name


def test_mail_names_each_failed_check():
    body = render_html(parse_log(FAILED_TEST), CTX)
    assert "3 rijen in raw_olist_orders hebben geen order_id" in body
    assert "models/edw_dv_bdv/sources_olist.yml:40:17" in body and "source_not_null_raw_olist_raw_olist_orders_order_id" in body
    assert "Niet gerund omdat een eerdere stap mislukte" in body and "mart_olist_management_kpi" in body
    assert "6 stappen zijn niet gerund: 4 tabellen en 2 checks" in body
    body = render_html(parse_log(FAILED_TESTS_DB_ERROR), CTX)
    body = html.unescape(body)
    assert "De check 'event_id in raw_carrier_tracking_events mag nooit leeg zijn' kon niet afgewerkt worden" in body
    assert "Table 'dev_landing.raw_carrier_tracking_events' doesn't exist" in body
    assert "<script" not in body and "<style" not in body and "href=" in body


def test_subject():
    assert render_subject(parse_log(FAILED_TEST), CTX) == (
        "[development] Nachtelijke load mislukt: 1 check mislukt, 6 stappen niet gerund")
    assert render_subject(parse_log(FAILED_TESTS_DB_ERROR), CTX) == (
        "[development] Nachtelijke load mislukt: 2 checks mislukt, 8 stappen niet gerund")
    assert render_subject(parse_log(PARSE_ERROR), {**CTX, "environment": "prd"}) == (
        "[productie] Nachtelijke load mislukt: 1 tabel mislukt")
    assert render_subject(parse_log("   Skipped model a (table) [1 of 2]\n   Failed model b (table) [2 of 2 in 0.1s]\n"),
                          {**CTX, "environment": "val"}) == "[validatie] Nachtelijke load mislukt: 1 tabel mislukt, 1 stap niet gerund"
    assert render_subject(parse_log(""), CTX) == "[development] Nachtelijke load mislukt"
    assert render_subject(parse_log(""), {**CTX, "dag_id": "dbt_reload"}) == "[development] dbt_reload mislukt"


def test_one_set_of_counts_everywhere():
    for log, expected in ((FAILED_TEST, "42 gelukt, 1 mislukt, 6 niet gerund"),
                          (FAILED_TESTS_DB_ERROR, "39 gelukt, 2 mislukt, 8 niet gerund"),
                          (WARNED_TEST, "48 gelukt, 1 met waarschuwing, 0 mislukt, 0 niet gerund")):
        report = parse_log(log)
        assert summary_line(report) == expected
    t = tally(parse_log(FAILED_TEST))
    assert (t.succeeded, t.failed, t.failed_checks, t.failed_tables, t.not_run) == (42, 1, 1, 0, 6)
    body = render_html(parse_log(FAILED_TEST), CTX)
    text = re.sub(r"<[^>]+>", "", body)
    assert "42 gelukt, 1 mislukt, 6 niet gerund" in text and "42 gelukt" in text and "43 gelukt, omdat" in text
    assert "1 hook" in count_note(parse_log(FAILED_TEST)) and count_note(parse_log("")) == ""
    assert text.count("Wat mislukte") == 1 and "Omgeving: development" in text
    assert "Details" not in text                          # no log message for this failure: the cell is left out
    assert "Details" in re.sub(r"<[^>]+>", "", render_html(parse_log(FAILED_TESTS_DB_ERROR), CTX))


def test_belgian_number_notation():
    from utils.dbt_failure_report import number_nl
    assert [number_nl(n) for n in (0, 42, 3432, 1234567, 92.8, 1234.5)] == ["0", "42", "3.432", "1.234.567", "92,8", "1.234,5"]
    assert describe_failure(Finding("unique_fct_olist_orders_order_id", "test", failures=1234)).startswith("1.234 waarden")


def test_plain_sentences():
    def sentence(node, rows):
        return describe_failure(Finding(node, "test", failures=rows))
    assert sentence("source_not_null_raw_olist_raw_olist_orders_order_id", 3) == (
        "3 rijen in raw_olist_orders hebben geen order_id")
    assert sentence("source_not_null_raw_olist_raw_olist_orders_order_id", 1) == (
        "1 rij in raw_olist_orders heeft geen order_id")
    assert sentence("unique_fct_olist_orders_order_id", 5) == (
        "5 waarden van order_id komen meer dan één keer voor in fct_olist_orders")
    assert sentence("source_accepted_values_raw_olist_raw_olist_orders_order_status__processing__shipped", 2) == (
        "2 rijen in raw_olist_orders hebben een waarde in order_status die niet toegelaten is")
    assert sentence("relationships_fct_olist_orders_customer_id__customer_id__ref_dim_olist_customers_", 4) == (
        "4 rijen in fct_olist_orders verwijzen naar een rij in dim_olist_customers die niet bestaat")
    assert sentence("assert_carrier_events_delivered", 2) == "Carrier events delivered: 2 rijen"
    assert sentence("assert_carrier_events_delivered", None) == (
        "De check 'carrier events delivered' kon niet afgewerkt worden")


def test_run_results_override_and_staleness(tmp_path):
    # The success entry is a real entry of /mnt/dbt/target/run_results.json (dbt 2.0.11); the fail/warn entries use
    # the status values and `failures` strings that dbt_run_results holds for failed and warned tests.
    path = tmp_path / "run_results.json"
    path.write_text(json.dumps({"results": [
        {"unique_id": "model.dbt_swf.datum", "status": "success", "message": "Succeeded", "failures": None,
         "execution_time": 5.476284927},
        {"unique_id": "test.dbt_swf.source_not_null_raw_olist_raw_olist_orders_order_id.abc123", "status": "fail",
         "failures": 3, "message": "Got 3 results, configured to fail if != 0", "execution_time": 0.003},
        {"unique_id": "test.dbt_swf.dbt_utils_source_expression_is_true_x.def456", "status": "warn", "failures": 8,
         "execution_time": 0.01},
    ]}))
    report = merge_run_results(parse_log(FAILED_TEST), load_run_results(str(path)))
    assert report.errors[0].message.startswith("Got 3 results") and report.errors[0].failures == 3
    assert report.warnings[0].node == "dbt_utils_source_expression_is_true_x" and report.warnings[0].failures == 8
    assert report.counts == {"pass": 1, "warn": 1, "error": 1, "skip": 0}
    assert load_run_results(str(path), not_before=time.time() + 60) is None   # older than the run: ignored
    assert load_run_results(str(tmp_path / "missing.json")) is None


class FakeSMTP:
    sent = []

    def __init__(self, host, port, timeout=None):
        self.host, self.port = host, port

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def sendmail(self, sender, recipients, message):
        FakeSMTP.sent.append((self.host, self.port, sender, recipients, message))


ENV = {"DBT_REPORT_SMTP_HOST": "mailpit", "DBT_REPORT_SMTP_PORT": "1025",
       "DBT_REPORT_MAIL_FROM": "airflow-dev@workshop.local", "DBT_FAILURE_EMAIL_RECIPIENTS": "a@x.local, b@x.local"}


def test_send_uses_environment_and_never_raises():
    FakeSMTP.sent.clear()
    assert send_report("subj", "<p>x</p>", env=ENV, smtp_factory=FakeSMTP)
    assert FakeSMTP.sent[0][:4] == ("mailpit", 1025, "airflow-dev@workshop.local", ["a@x.local", "b@x.local"])
    assert send_report("subj", "<p>x</p>", env={}, smtp_factory=FakeSMTP) is False      # not configured

    def boom(*args, **kwargs):
        raise OSError("connection refused")
    assert send_report("subj", "<p>x</p>", env=ENV, smtp_factory=boom) is False


def test_build_and_send_survives_garbage(monkeypatch):
    for key, val in ENV.items():
        monkeypatch.setenv(key, val)
    monkeypatch.setattr("smtplib.SMTP", FakeSMTP)
    os.environ.pop("DBT_REPORT_SMTP_HOST")
    assert build_and_send("\x00garbage", CTX) is False
