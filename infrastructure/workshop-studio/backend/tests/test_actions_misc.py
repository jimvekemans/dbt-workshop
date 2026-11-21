import io
import tarfile

import pydantic
import pytest
import requests

from studio.actions import clients, mail, misc, precommit
from studio.actions.base import ACTIONS, CLEANERS, StepFailed
from studio.patches import Patch
from test_actions_support import make_ctx


def test_wait_shows_its_reason_and_sleeps(tmp_path):
    ctx, slept = make_ctx(tmp_path), []
    ctx.sleep = slept.append
    result = misc.wait(ctx, misc.WaitParams(seconds=5, because="Letting the load finish."))
    assert (
        slept == [5]
        and ctx.messages == ["Letting the load finish."]
        and result.summary == "5 seconden gewacht: Letting the load finish."
    )
    with pytest.raises(pydantic.ValidationError):
        misc.WaitParams(seconds=0)
    assert misc.wait(ctx, misc.WaitParams(seconds=1, because="Reading the numbers")).summary == "Reading the numbers."


def test_mail_clear_and_cleaner(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(
        requests, "delete", lambda url, **k: calls.append(url) or type("R", (), {"status_code": 200, "text": "ok"})()
    )
    ctx = make_ctx(tmp_path)
    assert mail.mail_clear(ctx, mail.Params()).summary == "De mailbox is leeg."
    mail.clean_mail(ctx, "mail")
    assert (
        calls == ["http://mailpit:8025/api/v1/messages"] * 2
        and "mail" in CLEANERS
        and ACTIONS["mail.clear"].label(None) == "De mailbox leegmaken"
    )


def test_mail_clear_unreachable_is_plain(tmp_path, monkeypatch):
    def boom(*a, **k):
        raise requests.ConnectionError("no route")

    monkeypatch.setattr(requests, "delete", boom)
    with pytest.raises(StepFailed):
        mail.clear_inbox(make_ctx(tmp_path))


BOUNCER_OUT = """sqlfmt...................................................................Passed
dbt parse................................................................Passed
dbt-bouncer..............................................................Failed
- check_model_has_semi_colon:0:stg_x
check_model_code_does_not_contain_regexp_pattern:2:stg_x
"""


def test_refusal_names_the_rule_in_plain_words():
    assert precommit.refusal_reasons(BOUNCER_OUT) == [
        "De code eindigt met een puntkomma.",
        "De code gebruikt een datumfunctie die het team verboden heeft (to_char).",
    ]
    assert precommit.refusal_reasons("sqlfmt....Failed\ndbt parse....Failed\n") == [
        "De code kon niet opgemaakt worden zoals de regels van het team vragen.",
        "Het project kan niet meer correct gelezen worden.",
    ]
    assert precommit.refusal_reasons("sqlfmt....Passed\ndbt-bouncer....Passed\n") == []
    assert precommit.refusal_reasons("dbt-bouncer....Failed\nregexp_pattern:3:x") == [
        "De code gebruikt een datumfunctie die het team verboden heeft (str_to_date)."
    ]


def project_archive():
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as tar:
        for name, text in {"workspace/models/a.sql": "select 1", "workspace/dbt_project.yml": "name: x"}.items():
            info = tarfile.TarInfo(name)
            info.size = len(text)
            tar.addfile(info, io.BytesIO(text.encode()))
    return buf.getvalue()


def test_patch_is_applied_to_a_copy_of_the_baseline_project():
    patch = Patch(
        "p",
        "t",
        "m",
        files={"models/b.sql": "select 2"},
        ops=[{"path": "models/a.sql", "find": "select 1", "replace": "select 3"}],
    )
    with tarfile.open(fileobj=io.BytesIO(precommit.patched_project(project_archive(), patch))) as tar:
        files = {m.name: tar.extractfile(m).read().decode() for m in tar.getmembers() if m.isfile()}
        owner = {m.uid for m in tar.getmembers()}
    assert files["workspace/models/a.sql"] == "select 3" and files["workspace/models/b.sql"] == "select 2"
    assert files["workspace/dbt_project.yml"] == "name: x" and owner == {1000}


def run_precommit(tmp_path, monkeypatch, output, expect):
    patches = tmp_path / "patches" / "p"
    patches.mkdir(parents=True, exist_ok=True)
    (patches / "models").mkdir(exist_ok=True)
    (patches / "models" / "b.sql").write_text("select 2")
    monkeypatch.setattr(clients, "project_tar", lambda ctx, image: project_archive())
    monkeypatch.setattr(
        clients, "run_container", lambda *a, **k: clients.Proc(1 if "Failed" in output else 0, output, "")
    )
    return precommit.precommit_run(make_ctx(tmp_path), precommit.PreCommitParams(patch="p", expect=expect))


def test_precommit_refusal_and_acceptance(tmp_path, monkeypatch):
    refused = run_precommit(tmp_path, monkeypatch, BOUNCER_OUT, "refused")
    assert refused.summary == "De robot weigerde de wijziging: ze overtreedt 2 van de regels van het team."
    assert len(refused.data["reasons"]) == 2
    tidied = run_precommit(tmp_path, monkeypatch, BOUNCER_OUT + "=== tidied 1\n", "refused")
    assert tidied.summary.startswith("De robot kuiste de opmaak van 1 bestand op en weigerde de wijziging")
    accepted = run_precommit(tmp_path, monkeypatch, "sqlfmt....Passed\n", "ok")
    assert accepted.summary == "De robot keurde de wijziging goed: ze volgt de regels van het team."
    with pytest.raises(StepFailed) as err:
        run_precommit(tmp_path, monkeypatch, BOUNCER_OUT, "ok")
    assert err.value.summary == "De robot weigerde de wijziging: ze overtreedt 2 van de regels van het team."
    assert "De code eindigt met een puntkomma." in err.value.details
    with pytest.raises(StepFailed):
        run_precommit(tmp_path, monkeypatch, "sqlfmt....Passed\n", "refused")
    with pytest.raises(StepFailed) as err:
        run_precommit(tmp_path, monkeypatch, "docker: no such image", "ok")
    assert err.value.summary == "De robot kon de wijziging niet nakijken."
