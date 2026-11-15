"""Plain words, ordering and counts of the second review: bars, run list, pipeline card, names, tasks, git sentences."""
from datetime import datetime

import pytest

from studio import evidence, names
from studio.actions import airflow, clients, git_scripts, gitlab, precommit
from studio.actions.base import Result
from studio.templating import TemplateContext, render_item
from test_actions_support import make_ctx


def rows(monkeypatch, names_, data):
    monkeypatch.setattr(clients, "read_rows", lambda s, q, limit=200, timeout=5, user="root": (names_, data))


# --- 1. bars ---------------------------------------------------------------------------------------------------

def test_bars_oldest_first_today_last_and_marked(tmp_path, monkeypatch):
    rows(monkeypatch, ["d", "v"], [["2026-03-01", 3], ["2026-02-27", 1], ["2026-02-28", 2]])   # newest first
    out = evidence.bars(make_ctx(tmp_path), evidence.BarsParams(query="q"))
    assert out["labels"] == ["2026-02-27", "2026-02-28", "2026-03-01"]
    assert out["series"][0]["values"] == [1, 2, 3]
    assert [b["today"] for b in out["bars"]] == [False, False, True]
    assert [b["label"] for b in out["bars"]] == ["27 Feb", "28 Feb", "1 Mar (today)"]


def test_bars_with_other_labels_keep_the_query_order(tmp_path, monkeypatch):
    rows(monkeypatch, ["d", "v"], [["tue", 1], ["mon", 2]])
    out = evidence.bars(make_ctx(tmp_path), evidence.BarsParams(query="q"))
    assert out["labels"] == ["tue", "mon"] and not any(b["today"] for b in out["bars"])


# --- 2. run list -----------------------------------------------------------------------------------------------

def night(**kw):
    base_ = dict(tables_built=28, tables_total=28, tables_not_run=0, tables_failed=0, tables_left_out=0, checks_passed=21,
                 checks_failed=0, checks_warned=0, checks_not_run=0, retry=False)
    return {**base_, **kw}


def test_run_summaries_count_by_status():
    assert evidence.run_summary(night()) == "28 of 28 tables built, 21 checks passed"
    assert evidence.run_summary(night(tables_built=24, tables_not_run=4, checks_passed=18, checks_not_run=2, checks_failed=1)) \
        == "24 of 28 tables built, 4 tables and 2 checks not run, 1 check failed"
    assert evidence.run_summary(night(tables_built=25, tables_total=25, tables_left_out=3, checks_passed=14)) \
        == "25 of 28 tables built, 3 left out on purpose, 14 checks passed"
    assert evidence.run_summary(night(checks_passed=20, checks_warned=1)) == "28 of 28 tables built, 20 checks passed, 1 warning"
    assert evidence.run_summary(night(tables_built=4, tables_total=4, checks_passed=3, retry=True)) \
        == "Retry: 4 tables built, 3 checks passed"
    assert evidence.run_summary(night(tables_built=1, tables_total=1, checks_passed=1, retry=True)) \
        == "Retry: 1 table built, 1 check passed"
    assert evidence.run_summary(night(tables_built=27, tables_failed=1)) == "27 of 28 tables built, 1 table failed"


def test_run_history_keeps_the_numbers_as_fields(tmp_path, monkeypatch):
    # started, built, tables not run, tables failed, checks passed, checks not run, checks failed, checks warned
    rows(monkeypatch, [], [(datetime(2026, 3, 1, 4), 24, 4, 0, 18, 2, 1, 0)])
    run = evidence.run_history(make_ctx(tmp_path), evidence.RunHistoryParams())["runs"][0]
    assert run["state"] == "bad" and run["summary"] == "24 of 28 tables built, 4 tables and 2 checks not run, 1 check failed"
    keys = ("tables_built", "tables_total", "tables_not_run", "tables_failed", "tables_left_out",
            "checks_passed", "checks_failed", "checks_warned", "checks_not_run", "retry")
    assert tuple(run[k] for k in keys) == (24, 28, 4, 0, 0, 18, 1, 0, 2, False)
    assert run["tables_built"] + run["tables_failed"] + run["tables_not_run"] == run["tables_total"]


def project_of(monkeypatch, total):
    monkeypatch.setattr(evidence, "tables_in_project", lambda settings: total)


def test_run_history_counts_the_tables_left_out_of_the_project(tmp_path, monkeypatch):
    project_of(monkeypatch, 28)
    rows(monkeypatch, [], [(datetime(2026, 3, 1, 4), 25, 0, 0, 14, 0, 0, 0)])
    run = evidence.run_history(make_ctx(tmp_path), evidence.RunHistoryParams())["runs"][0]
    assert (run["tables_total"], run["tables_left_out"]) == (25, 3)
    assert run["summary"] == "25 of 28 tables built, 3 left out on purpose, 14 checks passed"
    assert run["tables_total"] + run["tables_left_out"] == 28


def test_a_warning_is_amber_not_green(tmp_path, monkeypatch):
    project_of(monkeypatch, 28)
    rows(monkeypatch, [], [(datetime(2026, 3, 1, 4), 28, 0, 0, 20, 0, 0, 1)])
    run = evidence.run_history(make_ctx(tmp_path), evidence.RunHistoryParams())["runs"][0]
    assert run["state"] == "warn" and run["summary"] == "28 of 28 tables built, 20 checks passed, 1 warning"


def test_a_small_run_after_a_failed_night_is_a_retry(tmp_path, monkeypatch):
    project_of(monkeypatch, 28)
    rows(monkeypatch, [], [(datetime(2026, 3, 1, 4, 5), 4, 0, 0, 3, 0, 0, 0),
                           (datetime(2026, 3, 1, 4, 0), 24, 4, 0, 18, 2, 1, 0)])
    first, second = evidence.run_history(make_ctx(tmp_path), evidence.RunHistoryParams(limit=2))["runs"]
    assert first["retry"] and first["tables_left_out"] == 0 and first["summary"] == "Retry: 4 tables built, 3 checks passed"
    assert not second["retry"]


# --- 3. the automatic checks -----------------------------------------------------------------------------------

LOG = """2026-11-15T16:24:02.014831Z 01O     Failed test  unique_fct_olist_orders_order_id (models/im_dimensional/olist_core.yml:24:9) [2 of 6 in 0.3s]
2026-11-15T16:24:02.049141Z 01O Test failed (6862 failed row(s)): unique_fct_olist_orders_order_id
2026-11-15T16:24:02.049141Z 01O \x1b[31mTest failed (2 failed row(s)): assert_orders_one_row_each\x1b[0m
"""


def test_failed_checks_are_read_from_the_job_log():
    assert gitlab.failed_checks_in(LOG) == [
        {"kind": "check", "text": "Orders table: the order number appears only once", "rows": 6862,
         "details": "unique_fct_olist_orders_order_id"},
        {"kind": "check", "text": "Orders one row each", "rows": 2, "details": "assert_orders_one_row_each"},
    ]
    assert gitlab.failed_checks_in("Failed model x (table) [1 of 6]")[0]["kind"] == "table"


def test_a_failed_table_build_is_named_with_its_plain_reason():
    from pathlib import Path
    log = (Path(__file__).parent / "fixtures" / "verify_failed_model.log").read_text()   # a real failed verify job
    found = gitlab.failed_checks_in(log)[0]
    assert found["kind"] == "table" and found["rows"] is None
    assert found["text"] == "Orders table could not be built: the same order number would be stored more than once."
    assert "Duplicate entry" in found["details"] and "dbt1308" in found["details"]
    assert gitlab.plain_reason("Error 1062: Duplicate entry 'x' for key 'order_id'") == \
        "the same order number would be stored more than once"
    assert gitlab.plain_reason("Error 1062: Duplicate entry 'x' for key 'PRIMARY'", "stg_other") == \
        "the same row would be stored more than once"
    assert gitlab.plain_reason("Error 1146: Table 'a.b' doesn't exist") == "it needs something that does not exist"
    assert gitlab.plain_reason("Error 1142: SELECT command denied") == "the robot is not allowed to do this"
    assert gitlab.plain_reason("boom") == "the database reported an error"


class Api:
    def __init__(self, trace=LOG):
        self.trace = trace

    def jobs(self, pid):
        return [
            {"id": 3, "name": "deploy_prd", "stage": "deploy", "status": "manual", "started_at": None},
            {"id": 2, "name": "tag_dev", "stage": "tag", "status": "success", "started_at": "2026-11-15T10:01:00Z"},
            {"id": 1, "name": "verify_feature_to_dev", "stage": "verify", "status": "failed",
             "started_at": "2026-11-15T10:00:00Z"},
        ]

    def failed_job_tail(self, pid):
        return "tail"

    def p(self, path):
        return path

    def req(self, method, path, ok=()):
        return type("R", (), {"text": self.trace})()


def test_pipeline_card_runs_in_order_and_names_the_failed_check(tmp_path, monkeypatch):
    gitlab._CHECKS_OF_JOB.clear()
    monkeypatch.setattr(clients, "GitLabApi", lambda *a, **k: Api())
    monkeypatch.setattr(evidence, "find_pipeline", lambda *a, **k: {"id": 1, "status": "failed"})
    out = evidence.pipeline(make_ctx(tmp_path), evidence.PipelineParams.model_validate({"for": "ref:dev"}))
    assert [j["label"] for j in out["jobs"]] == ["Trying the change in a private copy", "Numbering the version",
                                                 "Putting the version live in production"]
    assert out["jobs"][2]["state_text"] == "Waiting for a person"
    assert out["failures"][0]["rows"] == 6862 and out["failures"][0]["details"].startswith("unique_")


def test_every_job_kind_has_one_label():
    assert gitlab.job_word("verify_release") == "Checking the whole release"
    assert gitlab.job_word("tag_main") == gitlab.job_word("tag_dev") == "Numbering the version"
    assert gitlab.job_word("build_image") == "Packaging the version"
    assert gitlab.job_word("deploy_val") == "Putting the version live in validation"


def test_merge_request_summaries_say_rehearsal(tmp_path, monkeypatch):
    gitlab._CHECKS_OF_JOB.clear()
    monkeypatch.setattr(gitlab, "GitLabApi", lambda *a, **k: Api())
    monkeypatch.setattr(gitlab, "snapshot", lambda *a, **k: {"tags": []})
    monkeypatch.setattr(gitlab, "find_pipeline", lambda *a, **k: {"id": 1, "status": "failed", "ref": "x"})
    ctx = make_ctx(tmp_path)
    result = gitlab.wait_pipeline(ctx, gitlab.WaitPipelineParams.model_validate({"for": "mr:demo/x", "expect": "failed"}))
    assert result.summary == "The automatic rehearsal failed, as expected: trying the change in a private copy."
    assert result.data["failures"][0]["rows"] == 6862
    assert "pipeline" not in result.data
    monkeypatch.setattr(gitlab, "find_pipeline", lambda *a, **k: {"id": 1, "status": "success", "ref": "x"})
    ok = gitlab.wait_pipeline(ctx, gitlab.WaitPipelineParams.model_validate({"for": "mr:demo/x"}))
    assert ok.summary == "The automatic rehearsal passed."


# --- 4. names --------------------------------------------------------------------------------------------------

def test_cells_use_plain_resource_and_model_names():
    cells, technical = names.plain_cells({"type": "unit_test", "t": "test", "m": "model", "s": "source",
                                          "name": "fct_olist_orders", "other": "int_web_sessions", "n": 5})
    assert cells == {"type": "check", "t": "check", "m": "table", "s": "feed", "name": "Orders table",
                     "other": "Web sessions table", "n": 5}
    assert technical == ["fct_olist_orders", "int_web_sessions"]
    assert names.model_title("dummy_reload_date_window") == "Practice rebuild table"
    assert names.model_title("order_id") is None
    assert names.model_title("raw_olist_orders") == "Orders feed"


def test_refusals_say_tables_in_the_environment():
    from studio.actions.sql import describe_refusal, technical_refusal
    text = "DELETE command denied to user 'svc_dev_dbt'@'h' for table 'prd_landing.t'"
    assert describe_refusal("svc_dev_dbt", text) == \
        "The database refused: the development robot may not change tables in production."
    assert "prd_landing" in technical_refusal("svc_dev_dbt", text)


@pytest.mark.parametrize("line,expected", [
    ("CREATE USER 'a'@'%' IDENTIFIED BY 'x1!';", "CREATE USER 'a'@'%' IDENTIFIED BY '••••••';"),
    ("ALTER USER a SET PASSWORD = 'x1';", "ALTER USER a SET PASSWORD = '••••••';"),
    ("  password: hunter2", "  password: '••••••'"),
    ("PASSWORD=abc123 other=1", "PASSWORD='••••••' other=1"),
    ("SELECT password_hash FROM t", "SELECT password_hash FROM t"),
    ("IDENTIFIED BY '••••••'", "IDENTIFIED BY '••••••'"),
])
def test_mask_secrets(line, expected):
    assert names.mask_secrets(line) == expected


# --- 7. / 8. task lists ----------------------------------------------------------------------------------------

def test_bookkeeping_tasks_collapse_to_one_row_with_the_worst_state():
    tasks = [{"task_id": "audit_environment_drift", "state": "success"},
             {"task_id": "g.merge_state__a", "state": "success"},
             {"task_id": "g.merge_state__b", "state": "failed"},
             {"task_id": "g.merge_state__c", "state": "success"}]
    out = airflow.describe_tasks(tasks)
    assert out[0]["label"] == "Checking for hand-made changes"
    assert out[1] == {"label": "Saving the results", "state": "failed", "state_text": "Failed", "count": 3}
    assert len(out) == 2


def test_state_of_the_first_attempt_is_said_after_a_retry():
    run = {"start_date": datetime(2026, 3, 1, 4, 5), "clear_number": 1}
    tasks = [{"task_id": "send_failure_report", "state": "success", "end_date": datetime(2026, 3, 1, 4, 1)},
             {"task_id": "dbt_build", "state": "success", "end_date": datetime(2026, 3, 1, 4, 6)}]
    out = airflow.describe_tasks(tasks, run)
    assert out[0]["state_text"] == "Done in the first attempt" and out[0]["state"] == "done"
    assert out[1]["state_text"] == "Done"
    assert airflow.describe_tasks(tasks, {**run, "clear_number": 0})[0]["state_text"] == "Done"


def test_result_evidence_has_no_run_id_in_its_data(tmp_path):
    ctx = make_ctx(tmp_path)
    ctx.last_result = airflow.judge("dbt_daily_load", "dev", "failed", "failed",
                                    [{"task_id": "dbt_build", "state": "failed"}], "studio__123_abc")
    out = evidence.result(ctx, evidence.ResultParams())
    assert "studio__" not in out["summary"] and "studio__" not in str(out["data"])
    assert "studio__" not in out["details"] or isinstance(out["details"], str)
    assert "Building all the reports" in out["summary"] + str(out["data"])


# --- 9. sentences ----------------------------------------------------------------------------------------------

def test_precommit_sentences_use_the_robot_and_the_teams_rules():
    out = precommit.refusal_reasons("dbt-bouncer....Failed\nregexp_pattern:3:x\ncheck_model_has_semi_colon")
    assert "The code ends with a semicolon." in out
    assert "It uses a date function the team has banned (str_to_date)." in out


def test_branch_words_for_the_release_demo():
    assert gitlab.branch_words("dev") == "the shared work in progress"
    assert gitlab.branch_words("release/2026-Q4") == "the copy for the release"
    assert gitlab.branch_words("main") == "production"


def test_versions_item_keeps_its_caption():
    item, env = render_item({"type": "versions", "title": "T", "caption": "Look here."},
                            TemplateContext("dev", today=lambda: datetime(2026, 3, 1).date()))
    assert env == "dev" and item["caption"] == "Look here."
    assert evidence.VersionsParams.model_validate({k: v for k, v in item.items() if k != "type"}).caption == "Look here."
