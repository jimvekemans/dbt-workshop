"""`toolbox.run` (a developer working in the toolbox), its clean-up, and the `id` of an action."""

from __future__ import annotations

import io
import tarfile
from dataclasses import replace

import pydantic
import pytest

from studio import names
from studio.actions import clients, toolbox
from studio.actions.base import StepFailed
from studio.engine import Engine
from studio.catalog import load_catalog
from studio.recorder import Recorder
from test_actions_support import make_ctx
from tests.conftest import FakeFeeder, TODAY, scenario, write_catalog


def params(commands, **over):
    return toolbox.ToolboxRunParams.model_validate({"as": "developer:pu_dev1", "commands": commands, **over})


def marked(index, code, seconds, body, changed=""):
    return f"@@begin {index}\n{body}\n@@end {index} {code} {seconds}\n{changed}"


def changed_file(path, before, after):
    return f"@@changes 0\n@@file {path}\n@@before\n{before}\n@@after\n{after}\n@@endfile\n@@endchanges 0\n"


OLD = "version: 2\nsources:\n  - name: landing\n    tables:\n      - name: raw_olist_returns\n        columns:\n          - name: order_id\n          - name: return_id\n"
NEW = OLD + "          - name: return_channel\n"
BUILD = "Finished 'build' successfully for target 'default' [11.7s]\nProcessed: 1 hook | 28 models | 21 tests\nSummary: 50 total | 50 success"


def fake_toolbox(monkeypatch, text):
    seen = []

    def run(ctx, p):
        seen.append(p)
        return clients.Proc(0, text, "")

    monkeypatch.setattr(toolbox, "run_in_toolbox", run)
    return seen


# --- parameters --------------------------------------------------------------------------------------------------


def test_the_parameters_are_checked_when_the_scenario_loads():
    assert params([{"run": "dbt ls"}]).user == "pu_dev1"
    assert params([{"run": "x", "say": "One line."}]).commands[0].say == ["One line."]
    robot = params([{"run": "dbt ls"}], **{"as": "service"})
    assert robot.user == "svc_dev_dbt"
    for bad in (
        {"as": "service", "auth": "vault", "commands": [{"run": "x"}]},
        {"as": "service", "auth": "file", "commands": [{"run": "x"}]},
        {"as": "robot", "commands": [{"run": "x"}]},
        {"as": "service", "env": "prd", "commands": [{"run": "x"}]},
        {"as": "developer:root", "commands": [{"run": "x"}]},
        {"as": "developer:pu_dev1", "commands": []},
        {"as": "developer:pu_dev1", "commands": [{"run": "  "}]},
        {"as": "developer:pu_dev1", "commands": [{"run": "x", "expect": "maybe"}]},
        {"as": "developer:pu_dev1", "commands": [{"run": "x", "say": "{password}"}]},
        {"as": "developer:pu_dev1", "commands": [{"run": "x", "other": 1}]},
        {"as": "developer:pu_dev1", "auth": "token", "commands": [{"run": "x"}]},
        {"as": "developer:pu_dev1", "env": "prd", "commands": [{"run": "x"}]},
        {"as": "developer:pu_dev1", "commands": [{"run": "x"}], "extra": 1},
    ):
        with pytest.raises(pydantic.ValidationError):
            toolbox.ToolboxRunParams.model_validate(bad)


# --- what goes into the container --------------------------------------------------------------------------------


def test_the_robot_connects_with_its_own_password_in_the_environment(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    calls, copied = [], []
    monkeypatch.setattr(clients, "deployed_tag", lambda c, env: "v1.0.0")
    monkeypatch.setattr(clients, "project_tar", lambda c, image: _tiny_project())
    monkeypatch.setattr(clients, "docker", lambda c, *a, env=None, **k: calls.append((a, env)) or clients.Proc(0, "", ""))
    monkeypatch.setattr(clients, "docker_bytes", lambda c, *a, data=None, **k: copied.append(data) or b"")
    monkeypatch.setattr(clients, "remove_container", lambda name: None)
    toolbox.run_in_toolbox(ctx, params([{"run": "dbt ls"}], **{"as": "service"}))
    create = calls[0]
    assert create[1] == {"SINGLESTORE_PASS": "svc-pw"} and "svc-pw" not in " ".join(create[0])
    assert "SINGLESTORE_USER=svc_dev_dbt" in create[0] and "DBT_DEPLOY_TARGET=dev" in create[0]
    assert b".devcontainer/.env" not in copied[0]


def test_files_of_the_repository_project_replace_those_of_the_package(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    (ctx.settings.dbt_project_dir / "macros" / "core").mkdir(parents=True)
    (ctx.settings.dbt_project_dir / "macros" / "core" / "x.sql").write_text("new")
    copied = []
    monkeypatch.setattr(clients, "deployed_tag", lambda c, env: "v1.0.0")
    monkeypatch.setattr(clients, "project_tar", lambda c, image: _tiny_project())
    monkeypatch.setattr(clients, "docker", lambda c, *a, env=None, **k: clients.Proc(0, "", ""))
    monkeypatch.setattr(clients, "docker_bytes", lambda c, *a, data=None, **k: copied.append(data) or b"")
    monkeypatch.setattr(clients, "remove_container", lambda name: None)
    toolbox.run_in_toolbox(ctx, params([{"run": "ls"}], use_repository=["macros/core/x.sql"]))
    with tarfile.open(fileobj=io.BytesIO(copied[0])) as tar:
        assert tar.extractfile("workspace/macros/core/x.sql").read() == b"new"
    with pytest.raises(StepFailed):
        toolbox.run_in_toolbox(ctx, params([{"run": "ls"}], use_repository=["macros/nope.sql"]))
    for bad in (["../x"], ["/etc/passwd"], [".git/config"], ["a//b"]):
        with pytest.raises(pydantic.ValidationError):
            params([{"run": "ls"}], use_repository=bad)


def test_the_settings_file_holds_the_password_or_only_the_vault_ids():
    by_file = toolbox.settings_file("pu_dev2", "file", "the-password")
    by_vault = toolbox.settings_file("pu_dev2", "vault", "the-password")
    assert "SINGLESTORE_PASS=the-password" in by_file and "KEYVAULT" not in by_file
    assert "the-password" not in by_vault and "SINGLESTORE_PASS" not in by_vault
    assert "KEYVAULT_ROLE_ID=workshop-role-id-pu_dev2" in by_vault
    assert "KEYVAULT_SECRET_ID=workshop-secret-id-pu_dev2" in by_vault
    assert "SINGLESTORE_USER=pu_dev2" in by_vault and "DBT_DEPLOY_TARGET=dev" in by_vault


def test_the_main_script_marks_every_command_and_lists_changes_only_when_asked():
    script = toolbox.main_script([toolbox.ToolboxCommand(run="a"), toolbox.ToolboxCommand(run="b", changes=True)])
    assert "@@begin 0" in script and "@@begin 1" in script and "commands/1.sh" in script
    assert script.count("@@changes") == 1 and "@@changes 1" in script and "cp -a models /tmp/snap" in script


def test_files_are_added_to_the_project_archive_owned_by_the_dbt_user():
    src = io.BytesIO()
    with tarfile.open(fileobj=src, mode="w") as tar:
        folder = tarfile.TarInfo("workspace")
        folder.type = tarfile.DIRTYPE
        tar.addfile(folder)
        info = tarfile.TarInfo("workspace/dbt_project.yml")
        info.size = 3
        tar.addfile(info, io.BytesIO(b"abc"))
    out = toolbox.add_files(src.getvalue(), {"commands/0.sh": b"ls\n", "workspace/.devcontainer/.env": b"A=1\n"})
    with tarfile.open(fileobj=io.BytesIO(out)) as tar:
        members = {m.name: m for m in tar}
        assert {"workspace/dbt_project.yml", "commands", "commands/0.sh", "workspace/.devcontainer",
                "workspace/.devcontainer/.env"} <= set(members)
        assert tar.extractfile("workspace/.devcontainer/.env").read() == b"A=1\n"
        assert (members["commands/0.sh"].uid, members["commands/0.sh"].gid) == (1000, 0)
        assert members["commands"].isdir()


def test_the_container_gets_the_password_by_environment_or_by_file_never_on_the_command_line(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    calls, copied = [], []
    monkeypatch.setattr(clients, "deployed_tag", lambda c, env: "v1.0.0")
    monkeypatch.setattr(clients, "project_tar", lambda c, image: _tiny_project())
    monkeypatch.setattr(clients, "docker", lambda c, *a, env=None, **k: calls.append((a, env)) or clients.Proc(0, "", ""))
    monkeypatch.setattr(clients, "docker_bytes", lambda c, *a, data=None, **k: copied.append(data) or b"")
    monkeypatch.setattr(clients, "remove_container", lambda name: None)
    toolbox.run_in_toolbox(ctx, params([{"run": "dbt ls"}], auth="password"))
    create = calls[0]
    assert create[1] == {"SINGLESTORE_PASS": "dev-pw"} and "dev-pw" not in " ".join(create[0])
    assert "SINGLESTORE_PASS" in create[0] and "SINGLESTORE_USER=pu_dev1" in create[0]
    assert b".devcontainer/.env" not in copied[0]
    calls.clear(), copied.clear()
    toolbox.run_in_toolbox(ctx, params([{"run": "dbt ls"}], auth="vault"))
    assert calls[0][1] is None and "SINGLESTORE_PASS" not in calls[0][0] and "dev-pw" not in " ".join(calls[0][0])
    assert b".devcontainer/.env" in copied[0] and b"dev-pw" not in copied[0]
    calls.clear(), copied.clear()
    toolbox.run_in_toolbox(ctx, params([{"run": "dbt ls"}], auth="file"))
    assert calls[0][1] is None and b"SINGLESTORE_PASS=dev-pw" in copied[0]


def _tiny_project() -> bytes:
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w") as tar:
        folder = tarfile.TarInfo("workspace")
        folder.type = tarfile.DIRTYPE
        tar.addfile(folder)
    return out.getvalue()


# --- reading what happened ---------------------------------------------------------------------------------------


def test_the_marked_output_is_split_per_command():
    text = marked(0, 0, "6.40", BUILD) + marked(1, 1, "0.20", "vdp: 'dbt build' without a selector")
    ran = toolbox.parse(text)
    assert ran[0].code == 0 and ran[0].seconds == 6.4 and ran[0].ended and "Processed" in ran[0].output
    assert ran[1].code == 1 and ran[1].lines == ["vdp: 'dbt build' without a selector"]


def test_the_facts_carry_their_nouns():
    ran = toolbox.parse(marked(0, 0, "6.40", BUILD))[0]
    facts = toolbox.facts_of(ran, "pu_dev2", None)
    assert (facts["tables"], facts["checks"], facts["time"]) == ("28 tabellen", "21 checks", "12 seconden")
    assert facts["user"] == "developer twee" and facts["account"] == "pu_dev2" and facts["last"] == "Summary: 50 total | 50 success"
    one = toolbox.parse(marked(0, 0, "0.30", "Processed: 1 hook | 1 model"))[0]
    facts = toolbox.facts_of(one, "pu_dev1", None)
    assert facts["tables"] == "1 tabel" and facts["time"] == "minder dan een seconde"
    assert toolbox.time_words(1.2) == "1 seconde"


def test_changes_to_source_descriptions_are_counted_and_put_in_plain_lines():
    adding = toolbox.describe_changes([("sources/sources_landing.yml", OLD, NEW)])
    assert adding.added == {"raw_olist_returns": ["return_channel"]} and (adding.before, adding.after) == (2, 3)
    assert toolbox.change_lines(adding) == ["Toegevoegd aan retourenfeed: retourkanaal"]
    new = toolbox.describe_changes([("sources/sources_landing.yml", "", OLD)])
    assert list(new.new_feeds) == ["raw_olist_returns"] and new.before == 0
    assert toolbox.change_lines(new) == ["Nieuwe feed beschreven: retourenfeed", "Kolom: bestelnummer", f"Kolom: {names.column_words('return_id')}"]
    assert toolbox.describe_changes([("x.yml", "{broken", "also: [")]).files == ["x.yml"]


# --- the action --------------------------------------------------------------------------------------------------


def test_a_forgotten_selection_without_the_guard_and_with_it(tmp_path, monkeypatch):
    text = marked(0, 0, "6.4", BUILD) + marked(1, 1, "0.2", "vdp: 'dbt build' without a selector would build the whole project.")
    fake_toolbox(monkeypatch, text)
    p = params([
        {"run": "engine build", "say": "Without the guard, {tables} were built in {time}."},
        {"run": "dbt build", "expect": "refused", "say": "With the guard, nothing was built."},
    ])
    result = toolbox.toolbox_run(make_ctx(tmp_path), p)
    assert result.summary == "Without the guard, 28 tabellen were built in 12 seconden."
    assert result.data["reasons"] == ["With the guard, nothing was built."] and result.data["refused"] is True
    assert result.touched == {"toolbox:dev:pu_dev1"} and "exit code 1" in result.details


def test_the_robot_works_in_the_shared_tables_so_the_action_touches_no_copies_of_its_own(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, marked(0, 0, "1", "Finished 'build' successfully for target 'run' [0.4s]\nProcessed: 1 hook | 1 models | 0 tests"))
    ctx = make_ctx(tmp_path)
    result = toolbox.toolbox_run(ctx, params([{"run": "engine build", "say": "The robot built {tables}."}], **{"as": "service"}))
    assert result.summary == "The robot built 1 tabel." and result.touched == set()
    assert ctx.messages == ["De robot van development runt 1 commando in de toolbox"]
    assert "svc_dev_dbt" in result.details
    fake_toolbox(monkeypatch, marked(0, 1, "1", "boom"))
    with pytest.raises(StepFailed) as err:
        toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "engine build"}], **{"as": "service"}))
    assert err.value.touched == set()


def test_a_command_that_was_to_be_refused_but_was_not_fails_the_step_and_still_reports_what_it_touched(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, marked(0, 0, "1", "ok"))
    with pytest.raises(StepFailed) as err:
        toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "dbt build", "expect": "refused"}]))
    assert "moeten weigeren" in err.value.summary and err.value.touched == {"toolbox:dev:pu_dev1"}


def test_refused_needs_the_toolbox_own_message_failed_needs_only_a_bad_exit_code(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, marked(0, 1, "1", "Finished 'test' with 1 error"))
    with pytest.raises(StepFailed):
        toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "dbt test", "expect": "refused"}]))
    done = toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "dbt test", "expect": "failed", "say": "It stopped."}]))
    assert done.summary == "It stopped." and done.data["refused"] is True
    fake_toolbox(monkeypatch, marked(0, 1, "1", "boom"))
    with pytest.raises(StepFailed) as err:
        toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "dbt ls"}]))
    assert "met problemen geëindigd" in err.value.summary


def test_a_command_that_did_not_run_to_its_end_fails_the_step(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, "@@begin 0\nhalf")
    with pytest.raises(StepFailed) as err:
        toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "dbt ls"}]))
    assert "niet afgeraakt" in err.value.summary


def test_the_answer_must_hold_the_text_the_step_needs(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, marked(0, 0, "1", "account\npu_dev2@%"))
    who = {"run": "who", "must_contain": ["{account}"], "say": "The database saw {user} connect."}
    with pytest.raises(StepFailed) as err:
        toolbox.toolbox_run(make_ctx(tmp_path), params([who]))
    assert "niet het antwoord" in err.value.summary and "pu_dev1" in err.value.details
    fake_toolbox(monkeypatch, marked(0, 0, "1", "account\npu_dev1@%"))
    assert toolbox.toolbox_run(make_ctx(tmp_path), params([who])).summary == "The database saw developer één connect."


def test_a_fact_the_output_lacks_is_a_failed_step_not_a_broken_sentence(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, marked(0, 0, "1", "nothing useful"))
    with pytest.raises(StepFailed) as err:
        toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "dbt build", "say": "Built {tables}."}]))
    assert "kon niet gelezen worden" in err.value.summary


def test_what_a_command_changed_is_reported_in_plain_lines(tmp_path, monkeypatch):
    text = marked(0, 0, "0.1", "added dev_landing.raw_olist_returns (2 columns)", changed_file("sources/sources_landing.yml", "", OLD))
    fake_toolbox(monkeypatch, text)
    result = toolbox.toolbox_run(make_ctx(tmp_path), params([
        {"run": "vdp sources add x", "changes": True, "say": "It described {new_feeds} with {columns_added} in {changed_files}."}]))
    assert result.summary == "It described 1 nieuwe feed with 2 kolommen in 1 bestand."
    assert result.data["reasons"] == ["Nieuwe feed beschreven: retourenfeed", "Kolom: bestelnummer", f"Kolom: {names.column_words('return_id')}"]


def test_no_password_and_no_vault_id_is_ever_in_the_output(tmp_path, monkeypatch):
    leak = "SINGLESTORE_PASS=dev-pw KEYVAULT_SECRET_ID=workshop-secret-id-pu_dev1 role workshop-role-id-pu_dev1"
    fake_toolbox(monkeypatch, marked(0, 0, "1", leak))
    result = toolbox.toolbox_run(make_ctx(tmp_path), params([{"run": "cat .devcontainer/.env", "say": "Read: {last}"}]))
    shown = result.summary + result.details + str(result.data)
    assert "dev-pw" not in shown and "workshop-secret-id" not in shown and "workshop-role-id" not in shown
    assert "***" in result.details


def test_the_action_works_on_development_only(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, marked(0, 0, "1", "ok"))
    with pytest.raises(StepFailed):
        toolbox.toolbox_run(make_ctx(tmp_path, env="prd"), params([{"run": "dbt ls"}]))


def test_the_progress_and_label_use_plain_words(tmp_path, monkeypatch):
    fake_toolbox(monkeypatch, marked(0, 0, "1", "ok"))
    ctx = make_ctx(tmp_path)
    toolbox.toolbox_run(ctx, params([{"run": "dbt ls"}]))
    assert ctx.messages == ["Developer één runt 1 commando in de toolbox"]


# --- the clean-up ------------------------------------------------------------------------------------------------


class FakeCursor:
    def __init__(self, log, rows):
        self.log, self.rows = log, rows

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False

    def execute(self, sql, args=None):
        self.log.append(sql)

    def fetchall(self):
        return self.rows


class FakeConnection:
    def __init__(self, log, rows):
        self.log, self.rows = log, rows

    def cursor(self):
        return FakeCursor(self.log, self.rows)

    def close(self):
        self.log.append("closed")


def test_the_cleaner_drops_every_copy_of_the_developer_whichever_database_it_is_in(tmp_path, monkeypatch):
    log = []
    rows = [("dev_vdp_mgmt", "pu_dev1_dbt_run_results", "BASE TABLE"), ("dev_im_dimensional", "pu_dev1_mart", "VIEW"),
            ("dev_landing", "pu_dev10_other", "BASE TABLE"), ("dev_im_dimensional", "pu_dev2_mart", "BASE TABLE"),
            ("val_im_dimensional", "pu_dev1_mart", "BASE TABLE")]
    monkeypatch.setattr(clients, "s2_connect", lambda *a, **k: FakeConnection(log, rows))
    result = toolbox.clean_toolbox(make_ctx(tmp_path), "toolbox:dev:pu_dev1")
    drops = [line for line in log if line.startswith("drop")]
    assert drops == ["drop table if exists `dev_vdp_mgmt`.`pu_dev1_dbt_run_results`",
                     "drop view if exists `dev_im_dimensional`.`pu_dev1_mart`"]
    assert result.summary == "De testtabellen van developer één zijn verwijderd."


@pytest.mark.parametrize("resource", ["toolbox:prd:pu_dev1", "toolbox:dev:svc_dev_dbt", "toolbox:dev:pu_dev1; drop"])
def test_the_cleaner_refuses_what_it_does_not_know(tmp_path, resource):
    with pytest.raises(StepFailed):
        toolbox.clean_toolbox(make_ctx(tmp_path), resource)


def test_the_cleaner_runs_after_the_developer_cleaner_and_before_the_rest():
    from studio.actions import base
    order = base.CLEAN_ORDER
    assert order.index("developer") < order.index("toolbox") < order.index("ci")
    assert base.CLEANERS["toolbox"] is toolbox.clean_toolbox


# --- the id of an action -------------------------------------------------------------------------------------------


@pytest.mark.usefixtures("fake_registry")
def test_an_action_id_keeps_the_result_next_to_another_run_of_the_same_action(settings, stack, tmp_path):
    step = {"id": "both", "title": "Both", "button": "Go", "outcome": "Done.",
            "do": [{"action": "fake.set", "key": "one", "id": "first"}, {"action": "fake.set", "key": "two", "id": "second"},
                   {"action": "fake.set", "key": "three"}],
            "show": [{"type": "table", "title": "Rows", "query": "select 1"}]}
    sc = scenario("demo-ids")
    sc["demo"]["steps"] = [step, sc["demo"]["steps"][0] | {"id": "other"}]
    cfg = replace(settings, scenarios_dir=write_catalog(tmp_path / "ids", [sc]))
    eng = Engine(cfg, load_catalog(cfg, lambda: TODAY), Recorder(cfg.recordings_dir), lambda: FakeFeeder(stack))
    try:
        eng.prepare("demo-ids")
        eng.wait_idle(10)
        eng.run_step("demo-ids", "both", {})
        eng.wait_idle(10)
        assert eng.state()["demo"]["steps"][0]["status"] == "done"
        assert {k: v.summary for k, v in eng._results.items()} == {
            "fake.set": "Set three.", "first": "Set one.", "second": "Set two."}
    finally:
        eng.cancel()
        eng.wait_idle(5)
