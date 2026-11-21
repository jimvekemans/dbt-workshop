import pydantic
import pytest

from studio.actions import clients, dbt
from studio.actions.base import StepFailed
from test_actions_support import make_ctx


def test_identities():
    assert dbt.identity("service", "val") == dbt.Identity("svc_val_dbt", "run", "val")
    ci = dbt.identity("ci", "prd")
    assert (ci.user, ci.target, ci.image_tag_env, ci.extra) == ("svc_ci_dbt", "ci", "dev", ("--vars", "SCHEMA: demo"))
    dev = dbt.identity("developer:pu_dev1", "dev")
    assert (dev.user, dev.target, dev.dev_image) == ("pu_dev1", "default", True)


def test_command_line_gets_target_and_vars():
    assert dbt.dbt_args("build --select x", dbt.identity("service", "dev")) == [
        "dbt",
        "build",
        "--select",
        "x",
        "--target",
        "run",
    ]
    assert dbt.dbt_args("build", dbt.identity("ci", "dev")) == [
        "dbt",
        "build",
        "--target",
        "ci",
        "--vars",
        "SCHEMA: demo",
    ]
    assert dbt.dbt_args("run --target reload", dbt.identity("service", "dev")) == ["dbt", "run", "--target", "reload"]


def test_params_validation():
    assert (
        dbt.DbtRunParams.model_validate({"command": "build", "as": "developer:pu_dev2", "expect": "ok"}).expect
        == "success"
    )
    for bad in (
        {"command": "build", "as": "root"},
        {"command": "build", "expect": "maybe"},
        {"command": "build", "x": 1},
    ):
        with pytest.raises(pydantic.ValidationError):
            dbt.DbtRunParams.model_validate(bad)


def test_finished_line_is_the_last_one():
    out = "x\nFinished 'build' with 1 error\nmore\nFinished 'run' ok"
    assert dbt.finished_line(out) == "Finished 'run' ok" and dbt.finished_line("boom") is None


def fake_run(monkeypatch, code, text):
    monkeypatch.setattr(dbt, "run_dbt", lambda *a, **k: clients.Proc(code, text, ""))


@pytest.mark.parametrize(
    "who,touched", [("service", {"models:dev"}), ("ci", {"ci"}), ("developer:pu_dev1", {"developer:dev:pu_dev1"})]
)
def test_touched_per_identity(tmp_path, monkeypatch, who, touched):
    fake_run(monkeypatch, 0, "Finished 'build' successfully")
    result = dbt.dbt_run(make_ctx(tmp_path), dbt.DbtRunParams.model_validate({"command": "build", "as": who}))
    assert result.touched == touched and result.summary == "De build van de rapporten is zonder problemen afgelopen."


def test_expected_failure_succeeds_but_a_crash_does_not(tmp_path, monkeypatch):
    p = dbt.DbtRunParams(command="test", expect="failed")
    fake_run(monkeypatch, 1, "Finished 'test' with 2 errors")
    assert "zoals verwacht" in dbt.dbt_run(make_ctx(tmp_path), p).summary
    fake_run(monkeypatch, 2, "profile is broken")
    with pytest.raises(StepFailed) as err:
        dbt.dbt_run(make_ctx(tmp_path), p)
    assert "kon niet eens starten" in err.value.summary


def test_unexpected_outcomes_fail_with_plain_sentences(tmp_path, monkeypatch):
    fake_run(monkeypatch, 1, "Finished 'build' with 1 error")
    with pytest.raises(StepFailed) as err:
        dbt.dbt_run(make_ctx(tmp_path), dbt.DbtRunParams(command="build"))
    assert err.value.summary == "De build van de rapporten is met problemen geëindigd."
    fake_run(monkeypatch, 0, "Finished 'build'")
    with pytest.raises(StepFailed) as err:
        dbt.dbt_run(make_ctx(tmp_path), dbt.DbtRunParams(command="build", expect="failed"))
    assert "had moeten mislukken" in err.value.summary


def test_run_dbt_uses_the_deployed_image_and_copies_the_project_for_developers(tmp_path, monkeypatch):
    seen = {}
    monkeypatch.setattr(clients, "deployed_tag", lambda ctx, env: "v1.3.0")
    monkeypatch.setattr(clients, "project_tar", lambda ctx, image: seen.setdefault("project_from", image) and b"tar")
    monkeypatch.setattr(
        clients,
        "run_container",
        lambda ctx, image, command, **k: seen.update(image=image, command=command, **k) or clients.Proc(0, "", ""),
    )
    ctx = make_ctx(tmp_path)
    dbt.run_dbt(ctx, "dev", dbt.identity("service", "dev"), "build")
    assert seen["image"] == "dbt-vdp:v1.3.0" and seen["project"] is None
    assert seen["secret_env"] == {"SINGLESTORE_PASS": "svc-pw"} and seen["env"]["SINGLESTORE_USER"] == "svc_dev_dbt"
    dbt.run_dbt(ctx, "dev", dbt.identity("developer:pu_dev1", "dev"), "build")
    assert seen["image"] == dbt.DEV_IMAGE and seen["project"] == b"tar" and seen["project_from"] == "dbt-vdp:v1.3.0"
    assert seen["command"][-2:] == ["--target", "default"] and seen["secret_env"] == {"SINGLESTORE_PASS": "dev-pw"}


def test_cleaners_run_the_documented_commands(tmp_path, monkeypatch):
    commands = []
    monkeypatch.setattr(
        dbt,
        "run_dbt",
        lambda ctx, env, who, command, timeout=0: commands.append((env, who.user, command)) or clients.Proc(0, "", ""),
    )
    ctx = make_ctx(tmp_path)
    dbt.clean_models(ctx, "models:prd")
    dbt.clean_models(ctx, "models:dev:mart_olist_management_kpi")   # one shared table a rehearsal was allowed to build
    dbt.clean_developer(ctx, "developer:dev:pu_dev2")
    dbt.clean_ci(ctx, "ci")
    assert commands == [
        ("prd", "svc_prd_dbt", "build --full-refresh"),
        ("dev", "svc_dev_dbt", "run --select mart_olist_management_kpi --full-refresh"),
        ("dev", "pu_dev2", "run-operation cleanup_user_models"),
        ("dev", "svc_ci_dbt", "run-operation drop_nodes"),
    ]


def test_failed_cleanup_is_reported(tmp_path, monkeypatch):
    fake_run(monkeypatch, 1, "boom")
    with pytest.raises(StepFailed):
        dbt.clean_models(make_ctx(tmp_path), "models:dev")


def test_the_copies_a_rehearsal_left_behind_are_dropped_from_the_rehearsal_database(tmp_path, monkeypatch):
    statements = []

    class Cursor:
        def __enter__(self):
            return self

        def __exit__(self, *a):
            return False

        def execute(self, statement):
            statements.append(statement)

        def fetchall(self):
            return [("demo_x_fct_olist_orders", "BASE TABLE"), ("demo_x_view", "VIEW")]

    class Conn:
        def cursor(self):
            return Cursor()

        def close(self):
            statements.append("closed")

    monkeypatch.setattr(clients, "s2_connect", lambda *a, **k: Conn())
    result = dbt.clean_ci(make_ctx(tmp_path), "ci:copies")
    assert "drop table if exists `vdp_ci`.`demo_x_fct_olist_orders`" in statements
    assert "drop view if exists `vdp_ci`.`demo_x_view`" in statements and statements[-1] == "closed"
    assert result.summary == "De kopieën die de testruns achterlieten, zijn verwijderd."


def test_a_build_can_say_in_plain_words_how_much_it_covered(tmp_path, monkeypatch):
    out = "Finished 'build' successfully for target 'run' [8.3s]\nProcessed: 1 hook | 30 models | 36 tests\n"
    monkeypatch.setattr(dbt, "run_dbt", lambda *a, **k: clients.Proc(0, out, ""))
    ctx = make_ctx(tmp_path)
    plain = dbt.dbt_run(ctx, dbt.DbtRunParams(command="build"))
    assert "reasons" not in plain.data                                    # opt-in: other scenarios' cards stay as they were
    said = dbt.dbt_run(ctx, dbt.DbtRunParams(command="build", explain=True))
    assert said.data["reasons"] == ["30 tabellen gebuild en 36 checks gerund."]
    assert dbt.processed_words("Processed: 1 model | 1 test") == "1 tabel gebuild en 1 check gerund."
    assert dbt.processed_words("Processed: 2 seeds") == "0 tabellen gebuild en 0 checks gerund."
    assert dbt.processed_words("nothing here") is None
