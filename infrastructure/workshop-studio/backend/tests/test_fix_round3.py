"""The third fix round: counts and sentences of the nights, task rows, readable code blocks, the developer image
recipe, plain titles of every dbt check, and the throwaway containers that never stay behind."""
from __future__ import annotations

import subprocess
import threading
from dataclasses import replace
from datetime import datetime

import pytest

from studio import catalog, evidence, names
from studio.actions import airflow, clients, gitlab, images
from studio.actions.base import Cancelled, StepFailed
from studio.catalog import CatalogError, load_catalog, readable_lines
from studio.templating import TemplateContext, render
from test_actions_gitlab import FakeApi
from test_actions_support import make_ctx
from tests.conftest import TODAY, scenario, write_catalog

# --- R4: task rows ---------------------------------------------------------------------------------------------


def test_skipped_clean_up_in_a_run_without_failure_is_not_needed():
    tasks = [{"task_id": "dbt_build", "state": "success"},
             {"task_id": "send_failure_report", "state": "skipped"},
             {"task_id": "cleanup_scratch_table", "state": "skipped"},
             {"task_id": "pause_daily_load", "state": "skipped"}]
    out = {r["label"]: r for r in airflow.describe_tasks(tasks)}
    assert out["Sending the failure mail"]["not_needed"] and out["Removing the unfinished copy"]["not_needed"]
    assert "not_needed" not in out["Pausing the nightly load"] and "not_needed" not in out["Building all the reports"]


def test_in_a_run_with_a_failure_nothing_is_not_needed_and_later_steps_are_not_run():
    run = {"state": "failed"}
    tasks = [{"task_id": "dbt_build", "state": "failed"},
             {"task_id": "persist_artifacts_to_state", "state": "upstream_failed"},
             {"task_id": "dbt_source_freshness", "state": None},
             {"task_id": "send_failure_report", "state": "success"},
             {"task_id": "cleanup_temporary_artifacts", "state": "skipped"}]
    out = airflow.describe_tasks(tasks, run)
    assert [r["state_text"] for r in out] == ["Failed", "Not run", "Not run", "Done", "Skipped"]
    assert not any("not_needed" in r for r in out)
    assert all(r["state"] != "done" for r in out[1:3])


def test_repeated_parts_are_numbered():
    tasks = [{"task_id": f"batch_chunk_{n}", "state": "success"} for n in (2, 1, 3, 4)]
    tasks.append({"task_id": "atomic_swap", "state": "success"})
    out = airflow.describe_tasks(tasks)
    parts = [(r["part"], r["parts"]) for r in out if r["label"] == "Loading one part of the new copy"]
    assert parts == [(2, 4), (1, 4), (3, 4), (4, 4)]
    assert "part" not in out[-1]


def test_one_part_is_not_numbered():
    out = airflow.describe_tasks([{"task_id": "batch_chunk_1", "state": "success"}])
    assert "part" not in out[0]


# --- R13: readable code blocks ---------------------------------------------------------------------------------


def test_code_is_dedented_and_double_backticks_are_stripped():
    text = ['        """The ``orders`` table.', '', '        Rows with ``None``.', '        """', '        x = 1  # a ``note``',
            "        y = `a`"]
    assert readable_lines(text) == ['"""The orders table.', '', 'Rows with None.', '"""', 'x = 1  # a note', "y = `a`"]
    assert readable_lines(["    -- the ``key``", "      select 1"]) == ["-- the key", "  select 1"]
    assert readable_lines(["  password: 'abc'"]) == ["password: '••••••'"]


@pytest.mark.usefixtures("fake_registry")
def test_numbers_and_note_matching_stay_with_the_file(settings, tmp_path):
    sql_dir = tmp_path / "sql"
    sql_dir.mkdir()
    (sql_dir / "x.sql").write_text("  -- one\n      GRANT SELECT ON a TO b;\n    -- ``two``\n")
    block = scenario()["code"][0] | {"file": "../infrastructure/singlestore/x.sql", "from": "-- one", "to": "two",
                                      "notes": [{"line": "  GRANT", "says": "A grant."}]}
    ok = replace(settings, scenarios_dir=write_catalog(tmp_path / "ok", [scenario(code=[block])]), sql_dir=sql_dir)
    lines = load_catalog(ok, lambda: TODAY).blocks["demo-one"][0]["lines"]
    assert [(l["n"], l["text"]) for l in lines] == [(1, "-- one"), (2, "    GRANT SELECT ON a TO b;"), (3, "  -- two")]
    assert lines[1]["note"] == "A grant."


# --- item 9: the developer image's recipe ----------------------------------------------------------------------


@pytest.mark.usefixtures("fake_registry")
def test_code_blocks_may_come_from_the_dev_image_folder_only(settings, tmp_path):
    folder = tmp_path / "dev-image"
    (folder / "src").mkdir(parents=True)
    (folder / "Dockerfile").write_text("# one\nFROM base\n# two\n")
    (folder / "src" / "x").write_text("secret")
    block = scenario()["code"][0] | {"file": "dev-image/Dockerfile", "from": "# one", "to": "# two",
                                      "notes": [{"line": "FROM", "says": "The start."}]}
    ok = replace(settings, scenarios_dir=write_catalog(tmp_path / "ok", [scenario(code=[block])]), dev_image_dir=folder)
    shown = load_catalog(ok, lambda: TODAY).blocks["demo-one"][0]
    assert shown["gitlab_url"] == "" and [l["text"] for l in shown["lines"]][1] == "FROM base"
    for file in ("dev-image/src/x", "dev-image/../secrets", "dev-image/.hidden", "dev-image/", "dev-image/a\\b"):
        bad = replace(ok, scenarios_dir=write_catalog(tmp_path / "bad", [scenario(code=[block | {"file": file}])]))
        with pytest.raises(CatalogError):
            load_catalog(bad, lambda: TODAY)


def test_the_dev_image_folder_is_a_setting():
    from studio.config import Settings
    assert str(Settings().dev_image_dir) == "/workshop/dev-image"
    assert str(Settings.from_env({"DEV_IMAGE_DIR": "/x"}).dev_image_dir) == "/x"


# --- item 5, 6: table footer and unit, whole numbers with separators ------------------------------------------


def test_table_returns_footer_and_column_units(tmp_path, monkeypatch):
    monkeypatch.setattr(clients, "read_rows", lambda s, q, limit=200, timeout=5, user="root": (["n", "t"], [["a", 1.5]]))
    params = evidence.TableParams.model_validate({
        "query": "select 1", "footer": "Showing the 1 slowest",
        "columns": [{"key": "n", "label": "Name"}, {"key": "t", "label": "Time", "unit": "seconds"}]})
    out = evidence.table(make_ctx(tmp_path), params)
    assert out["footer"] == "Showing the 1 slowest"
    assert out["columns"] == [{"key": "n", "label": "Name"}, {"key": "t", "label": "Time", "unit": "seconds"}]
    assert evidence.table(make_ctx(tmp_path), evidence.TableParams(query="select 1"))["footer"] is None


@pytest.mark.parametrize("value, text", [(3432, "3,432"), (3432.0, "3,432"), (12, "12"), (2.5, "2.5"), (1234567, "1,234,567"),
                                         (0, "0")])
def test_a_whole_value_has_thousands_separators(value, text):
    ctx = TemplateContext("dev", {}, lambda: TODAY).with_value(value)
    assert render("{{ value }} customers", ctx) == f"{text} customers"
    assert str(render("{{ value }}", ctx)) == text


# --- R17: plain words ------------------------------------------------------------------------------------------


def test_every_dbt_check_of_the_project_has_a_plain_title():
    import json
    from pathlib import Path
    listed = json.loads((Path(__file__).parent / "fixtures" / "check_names.json").read_text())
    assert len(listed) > 30
    for name in listed:
        title = names.check_title(name)
        assert names.looks_like_check(name), name
        assert name not in title and "_" not in title and not title.endswith("..."), (name, title)
    assert names.check_title("dbt_utils_unique_combination_of_columns_dummy_reload_hkey_mod_hkey_record") == \
        "Practice rebuild table (by key): each combination of record key appears only once"
    assert names.check_title("dbt_utils_source_unique_combination_of_columns_raw_olist_raw_olist_order_payments_order_id"
                             "__payment_sequential") == \
        "Payments feed: each combination of order number and payment sequential appears only once"
    assert names.check_title("not_null_something_odd_col") == "Check on something odd col"


def test_progress_lines_and_results_say_it_plainly():
    assert names.plain_words("Reading the numbers.") == "Reading which version each environment runs."
    assert airflow.setting_name("dbt_env_vars") == "a setting of the nightly load"
    from studio.actions import feed, sql
    assert sql.answer_words([(3432,)]) == "The database answered."
    assert sql.answer_words([(1, 2)]) == "The database answered with 1 row."
    assert sql.answer_words([(1, 2), (3, 4)]) == "The database answered with 2 rows."
    from studio.actions.base import ACTIONS
    assert ACTIONS["feed.deliver"].label(feed.DeliverParams(days=3)) == "Sending the last 3 days of data again"


def test_a_version_pipeline_says_what_happened_to_the_version():
    jobs = [{"name": "tag_dev", "status": "success"}, {"name": "build_image", "status": "success"},
            {"name": "deploy_dev", "status": "success"}]
    assert gitlab.delivery_words(jobs) == "The version was numbered, packaged and put live in development."
    assert gitlab.delivery_words([{"name": "verify_feature_to_dev", "status": "success"}]) is None
    assert gitlab.delivery_words(jobs[:1]) is None


# --- item 8: a tool without a number -------------------------------------------------------------------------


def test_a_tool_without_a_number_is_the_teams_own_copy():
    own = "installed"
    assert images.shown("rule_checker", "0.0.0") == own and images.shown("rule_checker", "0.0.0+local") == own
    assert images.shown("rule_checker", images.NO_NUMBER) == own
    assert images.shown("rule_checker", "0.0.10") == "version 0.0.10"
    assert images.shown("layout_robot", None) == "not included"
    assert images.shown("report_builder", "2.0.11") == "version 2.0.11"
    assert images.parse("@@probe report_builder\nsomething without a number\n")["report_builder"] == images.NO_NUMBER


# --- item 10: throwaway containers ---------------------------------------------------------------------------


class Docker:
    """Records every `docker` call; `start` can be made to block until cancelled."""

    def __init__(self, monkeypatch, start=None, listed=""):
        self.calls, self.start, self.listed = [], start, listed
        monkeypatch.setattr(clients, "docker", self.docker)
        monkeypatch.setattr(clients, "docker_bytes", lambda ctx, *a, **k: b"")
        monkeypatch.setattr(subprocess, "run", self.run)

    def docker(self, ctx, *args, **kw):
        self.calls.append(args)
        if args[0] == "start" and self.start:
            self.start()
        return clients.Proc(0, "", "")

    def run(self, argv, **kw):
        self.calls.append(("subprocess", *argv[1:]))
        return subprocess.CompletedProcess(argv, 0, stdout=self.listed, stderr="")

    def removed(self):
        return [c[-1] for c in self.calls if c[:2] in (("subprocess", "rm"),)]

    def created(self):
        return [c[c.index("--name") + 1] for c in self.calls if c[0] == "create"]


def test_a_throwaway_container_is_named_labelled_and_removed(tmp_path, monkeypatch):
    docker = Docker(monkeypatch)
    clients.run_container(make_ctx(tmp_path), "img", ["true"])
    create = next(c for c in docker.calls if c[0] == "create")
    assert f"{clients.THROWAWAY_LABEL}=1" in create and clients.THROWAWAY_NAME.match(docker.created()[0])
    assert docker.removed() == docker.created()


def test_a_cancelled_step_still_removes_its_container(tmp_path, monkeypatch):
    def cancelled():
        raise Cancelled()

    docker = Docker(monkeypatch, start=cancelled)
    with pytest.raises(Cancelled):
        clients.run_container(make_ctx(tmp_path), "img", ["true"])
    assert docker.removed() == docker.created() and len(docker.created()) == 1


def test_a_failing_create_or_copy_still_removes_what_exists(tmp_path, monkeypatch):
    docker = Docker(monkeypatch)
    monkeypatch.setattr(clients, "docker_bytes", lambda *a, **k: (_ for _ in ()).throw(StepFailed("no copy")))
    with pytest.raises(StepFailed):
        clients.run_container(make_ctx(tmp_path), "img", ["true"], project=b"x")
    assert docker.removed() == docker.created()
    with pytest.raises(StepFailed):
        clients.project_tar(make_ctx(tmp_path), "img")
    assert len(docker.removed()) == 2


def test_docker_run_is_named_and_removed_even_when_cancelled(tmp_path, monkeypatch):
    def cancelled():
        raise Cancelled()

    docker = Docker(monkeypatch, start=None)
    monkeypatch.setattr(clients, "docker", lambda ctx, *a, **k: docker.calls.append(a) or (_ for _ in ()).throw(Cancelled()))
    with pytest.raises(Cancelled):
        clients.docker_run(make_ctx(tmp_path), "-u", "root", "img", "true")
    run = next(c for c in docker.calls if c[0] == "run")
    assert "--rm" in run and f"{clients.THROWAWAY_LABEL}=1" in run
    assert docker.removed() == [run[run.index("--name") + 1]]


def test_start_up_removes_only_the_studios_own_stale_containers(monkeypatch, tmp_path):
    listed = "studio-0123abcd\nstudio-feeder\nworkshop-studio\nsinglestore\nstudio-ffffffff\n"
    docker = Docker(monkeypatch, listed=listed)
    assert clients.sweep_throwaway() == ["studio-0123abcd", "studio-ffffffff"]
    ps = next(c for c in docker.calls if c[1] == "ps")
    assert f"label={clients.THROWAWAY_LABEL}" in ps and "-a" in ps
    assert docker.removed() == ["studio-0123abcd", "studio-ffffffff"]


def test_the_sweep_never_raises_without_docker(monkeypatch):
    monkeypatch.setattr(subprocess, "run", lambda *a, **k: (_ for _ in ()).throw(FileNotFoundError("docker")))
    assert clients.sweep_throwaway() == []


# --- item 11: clean-up of a job that was played ----------------------------------------------------------------


class JobApi(FakeApi):
    def __init__(self, statuses):
        super().__init__()
        self.statuses = list(statuses)

    def req(self, method, path, **kw):
        if method == "GET" and path.startswith("/jobs/") and "trace" not in path:
            status = self.statuses.pop(0) if len(self.statuses) > 1 else self.statuses[0]
            return type("R", (), {"status_code": 200, "json": lambda s: {"id": 9, "status": status, "user": None},
                                  "text": ""})()
        return type("R", (), {"status_code": 200, "json": lambda s: {"id": 9, "status": "running"}, "text": "boom"})()


def play(tmp_path, monkeypatch, statuses, cancel_at=None):
    ctx, api = make_ctx(tmp_path), JobApi(statuses)
    monkeypatch.setattr(gitlab, "connect", lambda c: (api, {"tags": []}))
    monkeypatch.setattr(gitlab, "find_job", lambda *a: {"id": 9, "status": "manual"})
    if cancel_at:
        def sleep(seconds):
            raise Cancelled()
        ctx.sleep = sleep
    return ctx, gitlab.PlayJobParams.model_validate({"for": "ref:main", "job": "deploy_prd"})


def touched_of(error) -> set[str]:
    return set(getattr(error, "touched", ()))


def test_a_failed_job_still_leaves_the_production_version_to_undo(tmp_path, monkeypatch):
    ctx, p = play(tmp_path, monkeypatch, ["failed"])
    with pytest.raises(StepFailed) as err:
        gitlab.play_job(ctx, p)
    assert "version:prd" in touched_of(err.value)


def test_a_cancelled_job_still_leaves_the_production_version_to_undo(tmp_path, monkeypatch):
    ctx, p = play(tmp_path, monkeypatch, ["running"], cancel_at=1)
    with pytest.raises(Cancelled) as err:
        gitlab.play_job(ctx, p)
    assert {"git", "version:prd"} <= touched_of(err.value)


def test_a_job_that_times_out_leaves_it_too(tmp_path, monkeypatch):
    ctx, p = play(tmp_path, monkeypatch, ["running"])
    p = p.model_copy(update={"timeout": 0})
    with pytest.raises(StepFailed) as err:
        gitlab.play_job(ctx, p)
    assert "version:prd" in touched_of(err.value)


def test_an_unexpected_error_after_playing_is_also_registered(tmp_path, monkeypatch):
    ctx, p = play(tmp_path, monkeypatch, ["running"])
    waits = []

    def wait_until(*a, **k):
        waits.append(1)
        if len(waits) > 1:
            raise RuntimeError("socket")
        return {"id": 9, "status": "manual"}

    monkeypatch.setattr(ctx, "wait_until", wait_until, raising=False)
    with pytest.raises(RuntimeError) as err:
        gitlab.play_job(ctx, p)
    assert "version:prd" in touched_of(err.value)


def test_the_engine_registers_what_an_unexpected_error_touched():
    import inspect
    from studio import engine
    source = inspect.getsource(engine.Engine._run_one)
    assert source.count('self._touch(set(getattr(e, "touched"') == 2   # a cancelled step and an unexpected error


def test_the_read_only_actions_register_nothing(tmp_path, monkeypatch):
    from studio.actions import docs  # noqa: F401 - read_description changes nothing and has no resource to undo
    for name in ("docs.read_description", "image.versions"):
        assert name in __import__("studio.actions.base", fromlist=["ACTIONS"]).ACTIONS
    import inspect
    assert "touched" not in inspect.getsource(images.versions) and "touched" not in inspect.getsource(docs.read_description)
