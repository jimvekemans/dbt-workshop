import json
from datetime import date, datetime
from decimal import Decimal

import pydantic
import pytest

from studio import evidence
from studio.actions import airflow, clients
from studio.actions.base import EVIDENCE, Result, StepFailed
from test_actions_support import make_ctx


def rows(monkeypatch, names, data):
    monkeypatch.setattr(clients, "read_rows", lambda s, q, limit=200, timeout=5, user="root": (names, data))


def test_every_provider_of_the_spec_is_registered():
    assert set(EVIDENCE) >= {
        "table",
        "bars",
        "number",
        "tasks",
        "pipeline",
        "mail",
        "versions",
        "result",
        "freshness",
        "run_history",
        "stack",
    }


def test_unknown_parameters_are_rejected():
    with pytest.raises(pydantic.ValidationError):
        evidence.NumberParams.model_validate({"query": "select 1", "units": "x"})
    assert evidence.PipelineParams.model_validate({"for": "ref:dev", "title": "t"}).for_ == "ref:dev"


def test_plain_makes_cells_json_safe():
    row = [Decimal("3"), Decimal("2.5"), datetime(2026, 3, 1, 4, 0), date(2026, 3, 1), b"x", None]
    assert json.dumps([evidence.plain(v) for v in row]) == '[3, 2.5, "2026-03-01T04:00:00", "2026-03-01", "x", null]'


def test_table_highlights_matching_rows(tmp_path, monkeypatch):
    rows(monkeypatch, ["order_id", "status"], [["a", "late"], ["b", "ok"]])
    p = evidence.TableParams.model_validate(
        {
            "query": "select 1",
            "columns": [{"key": "order_id", "label": "Order"}],
            "highlight": {"column": "status", "equals": "late", "as": "bad"},
        }
    )
    out = evidence.table(make_ctx(tmp_path), p)
    assert [r["state"] for r in out["rows"]] == ["bad", None] and out["columns"] == [
        {"key": "order_id", "label": "Order"}
    ]
    assert out["rows"][0]["cells"] == {"order_id": "a", "status": "late"}


def test_table_without_columns_uses_query_names(tmp_path, monkeypatch):
    rows(monkeypatch, ["n"], [])
    out = evidence.table(make_ctx(tmp_path), evidence.TableParams(query="select 1", empty="No orders."))
    assert out["columns"] == [{"key": "n", "label": "n"}] and out["rows"] == [] and out["empty"] == "No orders."


def test_bars_pivot_series_and_states(tmp_path, monkeypatch):
    rows(monkeypatch, ["d", "s", "v"], [["mon", "delivered", 5], ["mon", "unreadable", 1], ["tue", "delivered", 7]])
    out = evidence.bars(
        make_ctx(tmp_path), evidence.BarsParams(query="q", good=["delivered"], bad=["unreadable"], x_label="Day")
    )
    assert out["labels"] == ["mon", "tue"] and out["x_label"] == "Day"
    assert out["series"] == [
        {"name": "delivered", "state": "good", "values": [5, 7]},
        {"name": "unreadable", "state": "bad", "values": [1, 0]},
    ]


def test_bars_with_two_columns_is_one_series(tmp_path, monkeypatch):
    rows(monkeypatch, ["d", "v"], [["mon", 2]])
    assert evidence.bars(make_ctx(tmp_path), evidence.BarsParams(query="q"))["series"][0]["values"] == [2]


def test_number_with_comparison(tmp_path, monkeypatch):
    answers = iter([[[Decimal("98")]], [[100]]])
    monkeypatch.setattr(clients, "read_rows", lambda s, q, limit=200, timeout=5, user="root": (["n"], next(answers)))
    p = evidence.NumberParams.model_validate(
        {"query": "a", "unit": "orders", "compare_to": {"query": "b", "label": "Orders sent"}}
    )
    assert evidence.number(make_ctx(tmp_path), p) == {
        "value": 98,
        "unit": "orders",
        "compare": {"label": "Orders sent", "value": 100},
    }


def test_tasks_shows_friendly_labels_and_states(tmp_path, monkeypatch):
    run = {
        "run_id": "r",
        "state": "success",
        "start_date": datetime(2026, 3, 1, 4),
        "end_date": datetime(2026, 3, 1, 5),
    }
    monkeypatch.setattr(airflow, "latest_run", lambda *a: run)
    monkeypatch.setattr(
        airflow,
        "task_rows",
        lambda *a: [
            {"task_id": "dbt_build", "state": "failed"},
            {"task_id": "persist_artifacts_to_state", "state": "upstream_failed"},
        ],
    )
    out = evidence.tasks(make_ctx(tmp_path), evidence.TasksParams(dag="dbt_daily_load"))
    assert (
        out["run"]["state"] == "failed" and out["run"]["started"] == "2026-03-01T04:00:00+00:00"
    )  # the failed build decides
    assert out["tasks"][0] == {"label": "Building all the reports", "state": "failed", "state_text": "Failed"}
    assert out["tasks"][1]["state"] == "skipped"


def test_tasks_before_any_run(tmp_path, monkeypatch):
    monkeypatch.setattr(airflow, "latest_run", lambda *a: None)
    assert evidence.tasks(make_ctx(tmp_path), evidence.TasksParams(dag="dbt_release"))["run"] is None


class FakeApi:
    def jobs(self, pid):
        return [{"name": "verify_feature_to_dev", "status": "success"}, {"name": "deploy_prd", "status": "manual"}]


def test_pipeline_lists_jobs_in_words(tmp_path, monkeypatch):
    monkeypatch.setattr(clients, "GitLabApi", lambda s: FakeApi())
    monkeypatch.setattr(evidence, "find_pipeline", lambda api, spec, tags: {"id": 1, "status": "manual"})
    out = evidence.pipeline(make_ctx(tmp_path), evidence.PipelineParams.model_validate({"for": "ref:main"}))
    assert out["state"] == "done" and [j["label"] for j in out["jobs"]] == [
        "Trying the change in a private copy",
        "Putting the version live in production",
    ]
    assert out["jobs"][1]["state_text"] == "Waiting for a person"
    monkeypatch.setattr(evidence, "find_pipeline", lambda *a: None)
    assert (
        evidence.pipeline(make_ctx(tmp_path), evidence.PipelineParams.model_validate({"for": "ref:main"}))["state"]
        == "todo"
    )


def test_mail_returns_the_newest_matching_message(tmp_path, monkeypatch):
    seen = []

    def fake_mailpit(ctx, path, **params):
        seen.append((path, params))
        if path == "search":
            return {"messages": [{"ID": "abc", "Created": "2026-03-01T04:00:00Z"}]}
        return {
            "Subject": "Nightly load failed",
            "From": {"Name": "", "Address": "airflow@x"},
            "Date": "2026-03-01T04:01:00Z",
            "HTML": "<p>x</p>",
            "Text": "",
        }

    monkeypatch.setattr(evidence, "mailpit", fake_mailpit)
    out = evidence.mail(make_ctx(tmp_path), evidence.MailParams(match="failed"))
    assert out == {
        "found": True,
        "subject": "Nightly load failed",
        "from": "airflow@x",
        "received": "2026-03-01T04:01:00Z",
        "html": "<p>x</p>",
    }
    assert seen[0] == ("search", {"query": 'subject:"failed"', "limit": "1"}) and seen[1][0] == "message/abc"


def test_mail_falls_back_to_escaped_text_and_handles_no_mail(tmp_path, monkeypatch):
    answers = iter([{"messages": [{"ID": "a"}]}, {"Subject": "s", "Text": "<b>", "From": {}}, {"messages": []}])
    monkeypatch.setattr(evidence, "mailpit", lambda *a, **k: next(answers))
    assert evidence.mail(make_ctx(tmp_path), evidence.MailParams(match="s"))["html"] == "<pre>&lt;b&gt;</pre>"
    assert evidence.mail(make_ctx(tmp_path), evidence.MailParams(match="s")) == {
        "found": False,
        "empty": "No mail has arrived yet.",
    }


def test_versions_per_environment_with_unknown_when_unreachable(tmp_path, monkeypatch):
    def tag(ctx, env):
        if env == "val":
            raise StepFailed("No version is deployed in val yet.")
        return "v1.1.0" if env == "dev" else "2026-Q1.0"

    monkeypatch.setattr(clients, "deployed_tag", tag)
    out = evidence.versions(make_ctx(tmp_path), evidence.Show())["environments"]
    assert [(e["label"], e["tag"]) for e in out] == [
        ("Development", "v1.1.0"),
        ("Validation", None),
        ("Production", "2026-Q1.0"),
    ]


def test_result_is_the_last_summary(tmp_path):
    ctx = make_ctx(tmp_path)
    assert evidence.result(ctx, evidence.ResultParams())["summary"] == ""
    ctx.last_result = Result("The database refused: x may not change tables in y", data={"refused": True})
    assert evidence.result(ctx, evidence.ResultParams()) == {
        "summary": "The database refused: x may not change tables in y",
        "details": "",
        "data": {"refused": True},
    }


def test_age_words_and_source_labels():
    assert [evidence.age_words(s) for s in (None, 10, 7200, 3 * 86400, 86400, 600)] == [
        "never",
        "just now",
        "2 hours ago",
        "3 days ago",
        "1 day ago",
        "10 minutes ago",
    ]
    assert evidence.source_label("source.vdp.olist.raw_olist_orders") == "orders"


def test_freshness_reads_the_state_volume(tmp_path):
    ctx = make_ctx(tmp_path)
    assert evidence.freshness(ctx, evidence.Show())["sources"] == []
    path = ctx.settings.state_dir / "dev" / "mnt_dbt" / "target_freshness"
    path.mkdir(parents=True)
    (path / "sources.json").write_text(
        json.dumps(
            {
                "metadata": {"generated_at": "2026-03-01T04:00:00Z"},
                "results": [
                    {
                        "unique_id": "source.vdp.olist.raw_olist_orders",
                        "status": "pass",
                        "max_loaded_at_time_ago_in_s": 300,
                    },
                    {
                        "unique_id": "source.vdp.carrier.raw_carrier_tracking_events",
                        "status": "error",
                        "max_loaded_at_time_ago_in_s": 259200,
                    },
                ],
            }
        )
    )
    out = evidence.freshness(ctx, evidence.Show())
    assert out["generated_at"] == "2026-03-01T04:00:00Z"
    assert [(s["label"], s["state"], s["last_data"]) for s in out["sources"]] == [
        ("orders", "fresh", "5 minutes ago"),
        ("parcel scans", "very_late", "3 days ago"),
    ]


def test_run_history_summarises_each_run(tmp_path, monkeypatch):
    seen = []
    monkeypatch.setattr(
        clients,
        "read_rows",
        lambda s, q, limit=200, timeout=5, user="root": seen.append(q)
        or ([], [(datetime(2026, 3, 1, 4), 10, 0, 2, 40, 0, 0, 1), (datetime(2026, 2, 28, 4), Decimal("12"), 0, 0, 40, 0, 0, 0)]),
    )
    out = evidence.run_history(make_ctx(tmp_path, "val"), evidence.RunHistoryParams(limit=2))
    assert "val_vdp_mgmt.dbt_run_results" in seen[0] and "LIMIT 3" in seen[0]   # one more: the run before the oldest
    assert [r["state"] for r in out["runs"]] == ["bad", "good"]
    assert out["runs"][0]["summary"] == "10 of 12 tables built, 2 tables failed, 1 warning"
    assert out["runs"][1]["summary"] == "12 of 12 tables built, 40 checks passed"


def test_stack_reports_each_service_and_never_raises(tmp_path, monkeypatch):
    monkeypatch.setattr(evidence, "check_gitlab", lambda ctx: ("starting", "Code storage is still starting"))
    monkeypatch.setattr(evidence, "check_singlestore", lambda ctx: (_ for _ in ()).throw(RuntimeError("refused")))
    monkeypatch.setattr(evidence, "check_airflow", lambda ctx, env: ("up", "The scheduler is ready"))
    monkeypatch.setattr(evidence, "check_mailpit", lambda ctx: ("up", "ok"))
    services = {s["id"]: s for s in evidence.stack(make_ctx(tmp_path), evidence.Show())["services"]}
    assert list(services) == ["gitlab", "singlestore", "airflow-dev", "airflow-val", "airflow-prd", "mailpit", "feeder"]
    assert services["gitlab"]["status"] == "starting" and services["singlestore"]["status"] == "down"
    assert services["feeder"] == {
        "id": "feeder",
        "label": "Data delivery",
        "status": "up",
        "detail": "Delivering data; today is 2026-03-01",
    }


def test_number_counts_zero_for_a_table_that_does_not_exist(tmp_path, monkeypatch):
    import pymysql

    def read(s, q, limit=200, timeout=5, user="root"):
        if q == "gone":
            raise pymysql.err.ProgrammingError(1146, "Table 'x.y' doesn't exist")
        return ["n"], [[5]]

    monkeypatch.setattr(clients, "read_rows", read)
    p = evidence.NumberParams.model_validate({"query": "a", "compare_to": {"query": "gone", "label": "Before"}})
    assert evidence.number(make_ctx(tmp_path), p)["compare"] == {"label": "Before", "value": 0}


def test_freshness_reads_dbt_statuses_in_any_case(tmp_path):
    folder = tmp_path / "state" / "dev" / "mnt_dbt" / "target_freshness"
    folder.mkdir(parents=True)
    (folder / "sources.json").write_text(
        '{"results": [{"unique_id": "source.p.s.raw_carrier_events", "status": "Pass", '
        '"max_loaded_at_time_ago_in_s": 100}, {"unique_id": "source.p.s.raw_olist_orders", "status": "Warn"}]}'
    )
    out = evidence.freshness(make_ctx(tmp_path), evidence.Show())
    assert [s["state"] for s in out["sources"]] == ["fresh", "late"]


def test_pipeline_card_carries_what_was_rehearsed_only_when_asked(tmp_path, monkeypatch):
    found = {"tables": 1, "checks": 0, "tables_total": 28}
    monkeypatch.setattr(clients, "GitLabApi", lambda s: FakeApi())
    monkeypatch.setattr(evidence, "find_pipeline", lambda api, spec, tags: {"id": 1, "status": "success"})
    monkeypatch.setattr(evidence, "rehearsed", lambda settings, api, jobs: found)
    asked = evidence.pipeline(make_ctx(tmp_path), evidence.PipelineParams.model_validate({"for": "ref:dev", "rehearsed": True}))
    assert asked["rehearsed"] == found
    assert "rehearsed" not in evidence.pipeline(make_ctx(tmp_path), evidence.PipelineParams.model_validate({"for": "ref:dev"}))
    monkeypatch.setattr(evidence, "find_pipeline", lambda *a: None)
    assert evidence.pipeline(make_ctx(tmp_path), evidence.PipelineParams.model_validate({"for": "ref:dev", "rehearsed": True}))["rehearsed"] is None
