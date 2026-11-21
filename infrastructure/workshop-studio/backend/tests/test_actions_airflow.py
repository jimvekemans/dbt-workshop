import re
from datetime import datetime, timedelta
from pathlib import Path

import pydantic
import pytest

from studio.actions import airflow, clients
from studio.actions.base import ACTIONS, CLEANERS, StepFailed
from test_actions_support import make_ctx

DAGS = next(
    (
        p / "dbt-singlestore-workshop" / "airflow" / "dags"
        for p in Path(__file__).resolve().parents
        if (p / "dbt-singlestore-workshop").exists()
    ),
    Path("/nonexistent"),
)


def cli_recorder(monkeypatch, replies=None):
    calls = []

    def fake(ctx, env, *args, **kwargs):
        calls.append((env, args))
        return clients.Proc(0, (replies or {}).get(args[:2], ""), "")

    monkeypatch.setattr(clients, "airflow_cli", fake)
    return calls


@pytest.mark.skipif(not DAGS.exists(), reason="dbt project not mounted")
def test_every_static_task_id_of_the_dags_has_friendly_words():
    ids = set()
    for dag in DAGS.glob("*.py"):
        ids |= set(re.findall(r'task_id="([a-z_]+)"', dag.read_text()))
    assert ids and all(airflow.friendly_task(t) != "Bezig" for t in ids), sorted(
        t for t in ids if airflow.friendly_task(t) == "Bezig"
    )


def test_friendly_task_handles_groups_and_prefixes():
    assert airflow.friendly_task("batched_incremental_loads.batch_chunk_3") == "Eén deel van de nieuwe kopie laden"
    assert airflow.friendly_task("dbt_build") == "Alle rapporten builden"


def test_no_friendly_text_uses_jargon():
    words = " ".join(airflow.FRIENDLY_TASKS.values()) + " ".join(airflow.DAG_NAMES.values())
    assert not re.search(r"\b(DAG|MR|Variable|dbt)\b", words)


def test_set_variable_merges_dbt_env_vars(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch, {("variables", "get"): '{"A": "1", "B": "2"}'})
    result = airflow.set_variable(
        make_ctx(tmp_path), airflow.SetVariableParams(key="dbt_env_vars", value={"B": "9", "C": "3"})
    )
    sent = next(args for _, args in calls if args[:2] == ("variables", "set"))
    assert sent[3] == '{"A": "1", "B": "9", "C": "3"}'
    assert result.touched == {"variable:dev:dbt_env_vars"}
    assert result.summary == "Een instelling van de nachtelijke load is aangepast in development."


def test_other_variables_are_replaced_and_lists_are_json(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    airflow.set_variable(
        make_ctx(tmp_path), airflow.SetVariableParams(key="dbt_daily_load_dbtignore", value=["a/*.sql"])
    )
    assert calls[-1][1] == ("variables", "set", "dbt_daily_load_dbtignore", '["a/*.sql"]')


def test_the_version_variable_is_not_settable_here():
    with pytest.raises(pydantic.ValidationError):
        airflow.SetVariableParams(key="dbt_image_tag", value="v1")


def test_delete_and_cleaner_remove_the_variable(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    ctx = make_ctx(tmp_path)
    assert airflow.delete_variable(ctx, airflow.DeleteVariableParams(key="disabled_dbt_models")).touched == {
        "variable:dev:disabled_dbt_models"
    }
    airflow.clean_variable(ctx, "variable:val:dbt_env_vars")
    assert calls[-1] == ("val", ("variables", "delete", "dbt_env_vars"))
    assert "variable" in CLEANERS


def test_run_state_uses_the_key_task_of_the_nightly_load():
    run = {"state": "success"}
    failed_build = [
        {"task_id": "dbt_build", "state": "failed"},
        {"task_id": "persist_artifacts_to_state", "state": "success"},
    ]
    assert airflow.run_state("dbt_daily_load", run, failed_build) == "failed"
    assert airflow.run_state("dbt_release", run, failed_build) == "success"
    assert airflow.run_state("dbt_release", run, [{"task_id": "audit_environment_drift", "state": "failed"}]) == "failed"
    assert airflow.run_state("dbt_daily_load", {"state": None}, []) == "queued"


TASKS_FAILED = [{"task_id": "audit_environment_drift", "state": "success"}, {"task_id": "dbt_build", "state": "failed"}]
TASKS_OK = [{"task_id": "dbt_build", "state": "success"}]


def test_judge_success_failure_and_expectations():
    ok = airflow.judge("dbt_daily_load", "dev", "success", "success", TASKS_OK, "r1")
    assert ok.summary == "De nachtelijke load is zonder problemen afgelopen in development." and ok.touched == {
        "models:dev",
        "mail",
    }
    expected = airflow.judge("dbt_daily_load", "dev", "failed", "failed", TASKS_FAILED, "r1")
    assert "zoals verwacht" in expected.summary and "Alle rapporten builden" in expected.summary
    with pytest.raises(StepFailed) as err:
        airflow.judge("dbt_daily_load", "dev", "success", "failed", TASKS_FAILED, "r1")
    assert "Alle rapporten builden" in err.value.summary
    with pytest.raises(StepFailed) as err:
        airflow.judge("dbt_daily_load", "dev", "failed", "success", TASKS_OK, "r1")
    assert "had moeten mislukken" in err.value.summary


def test_run_dag_triggers_waits_and_shows_the_running_task(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    ctx = make_ctx(tmp_path)
    polls = iter([[{"task_id": "dbt_build", "state": "running"}], TASKS_OK])
    runs = iter([{"state": "running", "clear_number": 0}, {"state": "success", "clear_number": 0}])
    monkeypatch.setattr(airflow, "latest_run", lambda *a: next(runs))
    monkeypatch.setattr(airflow, "task_rows", lambda *a: next(polls))
    result = airflow.run_dag(ctx, airflow.RunDagParams(dag="dbt_release", conf={"dry_run": True}))
    verbs = [args[:2] for _, args in calls]
    assert verbs.count(("dags", "unpause")) == 2  # the release and the table rebuild it delegates to
    trigger = next(args for _, args in calls if args[:2] == ("dags", "trigger"))
    assert '"dry_run": true' in trigger[trigger.index("-c") + 1] and "-l" in trigger
    assert "Bezig met de release run: Alle rapporten builden" in ctx.messages
    assert result.summary.startswith("De release run is zonder problemen afgelopen")


def test_trigger_failure_is_plain(tmp_path, monkeypatch):
    monkeypatch.setattr(
        clients,
        "airflow_cli",
        lambda ctx, env, *a, **k: clients.Proc(1 if a[:2] == ("dags", "trigger") else 0, "", "DagNotFound"),
    )
    with pytest.raises(StepFailed) as err:
        airflow.trigger(make_ctx(tmp_path), "dev", "dbt_daily_load", {})
    assert err.value.summary == "De nachtelijke load kon niet gestart worden in development."


def test_retry_failed_clears_the_latest_run_and_waits_for_the_restart(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    logical = datetime(2026, 3, 1, 4, 0, 0)
    states = iter(
        [
            {"run_id": "r1", "state": "failed", "logical_date": logical, "clear_number": 0},  # the run to retry
            {"run_id": "r1", "state": "failed", "logical_date": logical, "clear_number": 0},  # not restarted yet
            {"run_id": "r1", "state": "success", "logical_date": logical, "clear_number": 1},
        ]
    )
    monkeypatch.setattr(airflow, "latest_run", lambda *a: next(states))
    monkeypatch.setattr(airflow, "task_rows", lambda *a: TASKS_OK)
    result = airflow.retry_failed(make_ctx(tmp_path), airflow.RetryFailedParams(dag="dbt_daily_load"))
    clear = next(args for _, args in calls if args[:2] == ("tasks", "clear"))
    assert "-f" in clear and "2026-03-01T04:00:00+00:00" in clear
    assert result.summary == "De nachtelijke load is zonder problemen afgelopen in development."


def test_retry_without_an_earlier_run_fails_plainly(tmp_path, monkeypatch):
    monkeypatch.setattr(airflow, "latest_run", lambda *a: None)
    with pytest.raises(StepFailed):
        airflow.retry_failed(make_ctx(tmp_path), airflow.RetryFailedParams(dag="dbt_daily_load"))


def test_actions_are_registered_with_labels():
    for name in ("airflow.set_variable", "airflow.delete_variable", "airflow.run_dag", "airflow.retry_failed"):
        assert name in ACTIONS
    assert ACTIONS["airflow.run_dag"].label(airflow.RunDagParams(dag="dbt_daily_load")) == "De nachtelijke load starten"


def test_task_rows_keep_only_the_newest_try_of_each_task(monkeypatch, tmp_path):
    rows = [
        {"task_id": "dbt_build", "state": "failed", "start_date": None, "end_date": None, "try_number": 1},
        {"task_id": "send_failure_report", "state": "success", "start_date": None, "end_date": None, "try_number": 1},
        {"task_id": "dbt_build", "state": "success", "start_date": None, "end_date": None, "try_number": 2},
    ]
    monkeypatch.setattr(clients, "airflow_query", lambda *a, **k: rows)
    got = airflow.task_rows(make_ctx(tmp_path, "dev"), "dev", "dbt_daily_load", "r1")
    assert [(t["task_id"], t["state"]) for t in got] == [("send_failure_report", "success"), ("dbt_build", "success")]


def test_unused_slices_are_left_out_of_the_task_list():
    tasks = [{"task_id": "init_green_table", "state": "success"}] + [
        {"task_id": "reload__dummy_reload_date_window", "state": s} for s in ("success", "skipped", "skipped")
    ] + [{"task_id": "cleanup_scratch_table", "state": "skipped"}]
    labels = [(t["label"], t["state"]) for t in airflow.describe_tasks(tasks)]
    assert labels.count(("Een grote tabel herbouwen zonder de lezers te storen", "skipped")) == 0
    assert ("De onafgewerkte kopie verwijderen", "skipped") in labels and len(labels) == 3


# --- the scheduler's own switches: pausing, parse-time settings, feeds that report in, runs that start by themselves ---

NIGHTLY_URIS = ["s2://workshop-dev/dev_landing/raw_olist_orders", "s2://workshop-dev/dev_landing/raw_carrier_tracking_events"]
SERIALIZED = {"dag": {"tasks": [{"__var": {"task_id": "dbt_build"}}, {"__var": {"task_id": "emit_asset_events"}},
                                {"__var": {"task_id": "extract_refreshed_tables"}}]}}


def fake_database(monkeypatch, **answers):
    """`clients.airflow_query` answering by a word of the statement; every statement is kept in the returned list."""
    seen = []

    def query(settings, env, sql, args=()):
        seen.append((env, sql, args))
        for word, rows in answers.items():
            if word.replace("_", " ") in sql.lower() or word in sql.lower():
                return list(rows() if callable(rows) else rows)
        return []

    monkeypatch.setattr(clients, "airflow_query", query)
    return seen


def test_new_jobs_and_tasks_have_plain_words():
    assert airflow.dag_name("dbt_on_demand") == "de build op aanvraag"
    assert airflow.dag_name("report_refreshed_notice") == "de verwittiging over ververste tabellen"
    for task in ("extract_refreshed_tables", "emit_asset_events", "send_refreshed_notice"):
        assert airflow.friendly_task(task) != "Bezig"
    assert airflow.friendly_task("dbt_build", "dbt_on_demand") == "De gevraagde tabellen builden"
    assert airflow.friendly_task("dbt_build", "dbt_daily_load") == "Alle rapporten builden"
    rows = airflow.describe_tasks([{"task_id": "dbt_build", "state": "success"}], None, "dbt_on_demand")
    assert rows[0]["label"] == "De gevraagde tabellen builden"
    assert airflow.setting_name("dbt_dataset_dependencies") != "een instelling"


def test_a_failed_on_demand_build_counts_as_a_failed_run():
    tasks = [{"task_id": "dbt_build", "state": "failed"}, {"task_id": "send_failure_report", "state": "success"}]
    assert airflow.run_state("dbt_on_demand", {"state": "success"}, tasks) == "failed"


def test_on_demand_runs_leave_the_job_paused_again(tmp_path, monkeypatch):
    cli_recorder(monkeypatch)
    monkeypatch.setattr(airflow, "latest_run", lambda *a: {"state": "success", "clear_number": 0})
    monkeypatch.setattr(airflow, "task_rows", lambda *a: TASKS_OK)
    ok = airflow.run_dag(make_ctx(tmp_path), airflow.RunDagParams(dag="dbt_on_demand", conf={"select": "tag:on-demand"}))
    assert "dag:dev:dbt_on_demand" in ok.touched and "models:dev" in ok.touched
    plain = airflow.run_dag(make_ctx(tmp_path), airflow.RunDagParams(dag="dbt_daily_load"))
    assert not any(t.startswith("dag:") for t in plain.touched)       # the nightly load is not paused again
    monkeypatch.setattr(airflow, "latest_run", lambda *a: {"state": "failed", "clear_number": 0})
    with pytest.raises(StepFailed) as err:
        airflow.run_dag(make_ctx(tmp_path), airflow.RunDagParams(dag="report_refreshed_notice"))
    assert "dag:dev:report_refreshed_notice" in err.value.touched


def test_the_schedule_and_the_tasks_are_read_from_what_the_scheduler_stored(tmp_path, monkeypatch):
    fake_database(monkeypatch, timetable_summary=[{"timetable_summary": "Asset"}], serialized_dag=[{"data": SERIALIZED}])
    assert airflow.dag_shape(make_ctx(tmp_path), "dev", "dbt_daily_load") == (
        "assets", {"dbt_build", "emit_asset_events", "extract_refreshed_tables"})
    import json as _json
    fake_database(monkeypatch, timetable_summary=[{"timetable_summary": "0 0 * * *"}],
                  serialized_dag=[{"data": _json.dumps(SERIALIZED)}])
    assert airflow.dag_shape(make_ctx(tmp_path), "dev", "dbt_daily_load")[0] == "clock"
    fake_database(monkeypatch)
    assert airflow.dag_shape(make_ctx(tmp_path), "dev", "dbt_daily_load") is None


def test_wait_for_dag_polls_until_the_new_setting_is_read(tmp_path, monkeypatch):
    stored = iter([("0 0 * * *", {"dbt_build"}), ("Asset", {"dbt_build"}), ("Asset", {"dbt_build", *airflow.EMIT_TASKS})])
    monkeypatch.setattr(airflow, "dag_shape", lambda *a: (lambda k, t: (("assets" if k == "Asset" else "clock"), t))(*next(stored)))
    ctx = make_ctx(tmp_path)
    result = airflow.wait_for_dag(ctx, airflow.WaitForDagParams(schedule="assets", has_tasks=list(airflow.EMIT_TASKS)))
    assert "wacht nu op de feeds in plaats van op een vast uur en meldt welke tabellen ververst zijn" in result.summary
    assert result.data == {"schedule": "assets", "announces": True} and not result.touched
    assert ctx.messages.count("Wachten tot de scheduler de nieuwe instelling inleest") == 2


def test_wait_for_dag_gives_up_plainly(tmp_path, monkeypatch):
    monkeypatch.setattr(airflow, "dag_shape", lambda *a: ("clock", {"dbt_build"}))
    ctx = make_ctx(tmp_path)
    ctx.wait_until = lambda probe, timeout, every=3, waiting="": (_ for _ in ()).throw(StepFailed(f"Gave up: {waiting}"))
    with pytest.raises(StepFailed) as err:
        airflow.wait_for_dag(ctx, airflow.WaitForDagParams(schedule="assets"))
    assert "heeft de nieuwe instelling niet ingelezen" in err.value.summary


def test_the_two_parse_time_settings_are_cleared_together_and_waited_for(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    waits = []
    monkeypatch.setattr(airflow, "wait_for_shape", lambda ctx, env, dag, **kw: waits.append((env, dag, kw)))
    airflow.clean_variable(make_ctx(tmp_path), "variable:dev:dbt_emit_asset_events")
    deleted = [args[2] for _, args in calls if args[:2] == ("variables", "delete")]
    assert sorted(deleted) == ["dbt_dataset_dependencies", "dbt_emit_asset_events"]
    assert waits == [("dev", "dbt_daily_load", {"schedule": "clock", "lacks": airflow.EMIT_TASKS, "missing_ok": True})]
    airflow.clean_variable(make_ctx(tmp_path), "variable:dev:dbt_env_vars")      # any other setting: no waiting
    assert len(waits) == 1


def test_wait_for_shape_accepts_a_dag_the_scheduler_has_not_read_when_asked_to(tmp_path, monkeypatch):
    monkeypatch.setattr(airflow, "dag_shape", lambda *a: None)
    assert airflow.wait_for_shape(make_ctx(tmp_path), "dev", "dbt_daily_load", schedule="clock", missing_ok=True) is None


def test_unpausing_marks_the_latest_run_and_the_cleaner_pauses_again(tmp_path, monkeypatch):
    calls = cli_recorder(monkeypatch)
    fake_database(monkeypatch, dag_run=[{"mark": 41}])
    ctx = make_ctx(tmp_path)
    result = airflow.unpause_dag(ctx, airflow.DagParams(dag="report_refreshed_notice"))
    assert calls[-1] == ("dev", ("dags", "unpause", "report_refreshed_notice"))
    assert result.touched == {"dag:dev:report_refreshed_notice"} and result.data == {"run_mark": 41}
    airflow.clean_dag(ctx, "dag:val:report_refreshed_notice")
    assert calls[-1] == ("val", ("dags", "pause", "report_refreshed_notice"))
    assert "dag" in CLEANERS and "dag" in __import__("studio.actions.base", fromlist=["x"]).CLEAN_ORDER


def feeds_database(monkeypatch, paused=False, needed=NIGHTLY_URIS, queued=()):
    return fake_database(
        monkeypatch,
        is_paused=[{"is_paused": paused}], dag_schedule_asset_reference=[{"uri": u} for u in needed],
        asset_dag_run_queue=[{"uri": u} for u in queued], dag_run=[{"mark": 7}],
    )


def posting(monkeypatch, answer='{"events": [11, 12, 13]}', code=0):
    sent = []

    def fake(ctx, *args, **kw):
        sent.append(args)
        return clients.Proc(code, "noise\n" + answer, "")

    monkeypatch.setattr(clients, "docker", fake)
    return sent


def test_feeds_report_in_one_by_one_and_the_lines_say_what_is_still_awaited(tmp_path, monkeypatch):
    feeds_database(monkeypatch)
    sent = posting(monkeypatch)
    params = airflow.ReportFeedsParams(feeds=["orders", "orders", "carrier"])
    result = airflow.report_feeds(make_ctx(tmp_path), params)
    assert result.data["reasons"] == [
        "Bestellingenfeed: gemeld. De nachtelijke load wacht nog op: Pakketscanfeed.",
        "Bestellingenfeed: opnieuw gemeld. De nachtelijke load wacht nog op: Pakketscanfeed.",
        "Pakketscanfeed: gemeld. Elke feed waarop de nachtelijke load wacht, heeft zich nu gemeld.",
    ]
    assert result.data["all_in"] is True and result.data["waiting"] == [] and result.data["run_mark"] == 7
    args = sent[0]
    assert args[:3] == ("exec", "workshop-airflow-dev", "python3") and "-c" in args
    assert args[args.index("-c") + 2] == '["s2://workshop-dev/dev_landing/raw_olist_orders", ' \
        '"s2://workshop-dev/dev_landing/raw_olist_orders", "s2://workshop-dev/dev_landing/raw_carrier_tracking_events"]'


def test_a_feed_that_reported_earlier_counts_and_a_partial_report_says_what_is_missing(tmp_path, monkeypatch):
    feeds_database(monkeypatch, queued=[NIGHTLY_URIS[0]])
    posting(monkeypatch, '{"events": [5]}')
    result = airflow.report_feeds(make_ctx(tmp_path), airflow.ReportFeedsParams(feeds=["orders"]))
    assert result.data["reasons"][0].startswith("Bestellingenfeed: opnieuw gemeld.") and result.data["waiting"] == ["Pakketscanfeed"]
    assert "wacht nog op 1 feed" in result.summary and not result.touched


def test_reporting_feeds_fails_plainly_when_nothing_would_notice(tmp_path, monkeypatch):
    posting(monkeypatch)
    feeds_database(monkeypatch, paused=True)
    with pytest.raises(StepFailed) as err:
        airflow.report_feeds(make_ctx(tmp_path), airflow.ReportFeedsParams(feeds=["orders"]))
    assert "staat uit" in err.value.summary
    feeds_database(monkeypatch, needed=[])
    with pytest.raises(StepFailed) as err:
        airflow.report_feeds(make_ctx(tmp_path), airflow.ReportFeedsParams(feeds=["orders"]))
    assert "wacht nog niet op feeds" in err.value.summary
    feeds_database(monkeypatch)
    posting(monkeypatch, '{"error": "unknown asset", "uri": "s2://workshop-dev/dev_landing/raw_web_events"}', code=3)
    with pytest.raises(StepFailed) as err:
        airflow.report_feeds(make_ctx(tmp_path), airflow.ReportFeedsParams(feeds=["web"]))
    assert "kent deze feed nog niet (raw_web_events)" in err.value.summary


def test_feed_words_and_params():
    assert airflow.feed_uri("val", "carrier") == "s2://workshop-val/val_landing/raw_carrier_tracking_events"
    assert airflow.feed_words(NIGHTLY_URIS[1]) == "Pakketscanfeed"
    with pytest.raises(pydantic.ValidationError):
        airflow.ReportFeedsParams(feeds=[])
    with pytest.raises(pydantic.ValidationError):
        airflow.ReportFeedsParams(feeds=["invoices"])


def test_waiting_for_a_run_that_started_by_itself_says_who_started_it(tmp_path, monkeypatch):
    when = datetime(2026, 11, 21, 17, 51, 16)
    answers = iter([[], [{"id": 8, "run_id": "asset_triggered__x", "run_type": "asset_triggered", "queued_at": when}]])
    seen = fake_database(monkeypatch, **{"from dag_run": lambda: next(answers),
                                         "max(e.timestamp)": [{"at": datetime(2026, 11, 21, 17, 51, 14)}]})
    monkeypatch.setattr(airflow, "wait_for_finish", lambda *a, **k: ("success", TASKS_OK))
    ctx = make_ctx(tmp_path)
    ctx.results["feeds"] = airflow.Result("x", data={"run_mark": 7})
    result = airflow.wait_for_run(ctx, airflow.WaitForRunParams(dag="dbt_daily_load", after="feeds"))
    assert result.data["reasons"] == ["De nachtelijke load is vanzelf gestart, 2 seconden nadat de laatste feed zich had gemeld.",
                                      "Geen persoon en geen klok heeft dit in gang gezet."]
    assert result.summary.endswith("Dat gebeurde vanzelf.") and result.data["started_by"] == "de feeds"
    for gap, words in ((0.4, "minder dan een seconde"), (1.2, "1 seconde"), (2.0, "2 seconden")):
        answers = iter([[{"id": 8, "run_id": "r", "run_type": "asset_triggered", "queued_at": when}]])
        fake_database(monkeypatch, **{"from dag_run": lambda: next(answers),
                                      "max(e.timestamp)": [{"at": when - timedelta(seconds=gap)}]})
        assert f"vanzelf gestart, {words} nadat de laatste feed" in airflow.wait_for_run(
            ctx, airflow.WaitForRunParams(dag="dbt_daily_load", after="feeds")).data["reasons"][0]
    assert next(a for _, sql, a in seen if "from dag_run" in sql.lower()) == ("dbt_daily_load", 7)
    assert "wachten tot de nachtelijke load vanzelf start".lower() in " ".join(ctx.messages).lower()


def test_a_notice_run_touches_only_the_inbox_and_its_switch(tmp_path, monkeypatch):
    fake_database(monkeypatch, **{"from dag_run": [{"id": 3, "run_id": "r", "run_type": "asset_triggered", "queued_at": None}],
                                  "max(e.timestamp)": [{"at": None}]})
    monkeypatch.setattr(airflow, "wait_for_finish", lambda *a, **k: ("success", TASKS_OK))
    ctx = make_ctx(tmp_path)
    ctx.results["on"] = airflow.Result("x", data={"run_mark": 1})
    result = airflow.wait_for_run(ctx, airflow.WaitForRunParams(dag="report_refreshed_notice", after="on"))
    assert result.touched == {"mail", "dag:dev:report_refreshed_notice"}
    assert result.data["reasons"][0] == "De verwittiging over ververste tabellen is vanzelf gestart, op gang gebracht door de feeds."


def test_waiting_for_a_run_needs_a_mark(tmp_path):
    with pytest.raises(StepFailed) as err:
        airflow.wait_for_run(make_ctx(tmp_path), airflow.WaitForRunParams(dag="dbt_daily_load", after="nothing"))
    assert "niets geeft aan vanaf wanneer" in err.value.summary


def test_announcements_name_the_asked_tables_and_count_the_rest(tmp_path, monkeypatch):
    monkeypatch.setattr(airflow, "latest_run", lambda *a: {"run_id": "r9"})
    rows = [{"uri": "s2://workshop-dev/dev_edw_dv_bdv/stg_olist_customers"},
            {"uri": "s2://workshop-dev/dev_im_dimensional/fct_olist_deliveries"}]
    seen = fake_database(monkeypatch, asset_event=rows)
    result = airflow.announcements(make_ctx(tmp_path), airflow.AnnouncementsParams(tables=["mart_olist_management_kpi", "fct_olist_deliveries"]))
    assert result.data["reasons"] == ["De nachtelijke load meldde 2 ververste tabellen.",
                                      "Managementrapport: niet gemeld.", "Leveringstabel: wel gemeld."]
    assert result.data["tables"] == {"mart_olist_management_kpi": False, "fct_olist_deliveries": True}
    assert seen[0][2] == ("dbt_daily_load", "r9")
    fake_database(monkeypatch)
    assert airflow.announcements(make_ctx(tmp_path), airflow.AnnouncementsParams()).data["reasons"] == ["De nachtelijke load meldde geen enkele tabel."]
    monkeypatch.setattr(airflow, "latest_run", lambda *a: None)
    with pytest.raises(StepFailed):
        airflow.announcements(make_ctx(tmp_path), airflow.AnnouncementsParams())


def test_new_actions_are_registered_with_labels():
    for name in ("airflow.wait_for_dag", "airflow.unpause_dag", "airflow.report_feeds", "airflow.wait_for_run", "airflow.announcements"):
        assert name in ACTIONS
    assert ACTIONS["airflow.unpause_dag"].label(airflow.DagParams(dag="report_refreshed_notice")) == "De verwittiging over ververste tabellen aanzetten"
    assert ACTIONS["airflow.report_feeds"].label(airflow.ReportFeedsParams(feeds=["orders"])) == "De feeds laten melden dat ze binnen zijn"
