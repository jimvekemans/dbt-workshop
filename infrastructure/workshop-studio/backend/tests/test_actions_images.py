import pydantic
import pytest

from studio.actions import clients, images
from studio.actions.base import StepFailed
from studio.actions.dbt import DEV_IMAGE
from test_actions_support import make_ctx

TOOLBOX = """@@probe report_builder
dbt-oss 2.0.11
@@probe connector
74194c79b0fc863a2a9d90f6f16fbeff553b786667f0ee5881e5066ba7a22e86  /usr/local/lib/dbt/libadbc_driver_singlestore.so
@@probe layout_robot
sqlfmt, version 0.30.0
@@probe rule_checker
0.0.0
@@probe helper_tool
vdp 0.1.0
@@probe pre_save_checks
pre-commit 4.7.0
"""
PRODUCTION = """@@probe report_builder
dbt-oss 2.0.11
@@probe connector
74194c79b0fc863a2a9d90f6f16fbeff553b786667f0ee5881e5066ba7a22e86  /usr/local/lib/dbt/libadbc_driver_singlestore.so
@@probe layout_robot
@@probe rule_checker
@@probe helper_tool
@@probe pre_save_checks
"""


def test_the_marked_output_is_read_per_tool():
    assert images.parse(TOOLBOX) == {
        "report_builder": "2.0.11", "connector": "74194c79b0fc", "layout_robot": "0.30.0", "rule_checker": "0.0.0",
        "helper_tool": "0.1.0", "pre_save_checks": "4.7.0"}
    got = images.parse(PRODUCTION)
    assert got["report_builder"] == "2.0.11" and got["layout_robot"] is None and got["pre_save_checks"] is None


def test_the_script_is_fixed_and_runs_every_named_probe():
    for name, (_, command, _) in images.PROBES.items():
        assert f"echo '@@probe {name}'; {command} 2>/dev/null | head -n 1 || true" in images.SCRIPT
    with pytest.raises(pydantic.ValidationError):
        images.ImageVersionsParams(probes=["rm -rf /"])
    with pytest.raises(pydantic.ValidationError):
        images.ImageVersionsParams(probes=[])
    with pytest.raises(pydantic.ValidationError):
        images.ImageVersionsParams(image="other")


def fake_images(monkeypatch, production=PRODUCTION):
    ran = []

    def run_container(ctx, image, command, **kw):
        ran.append((image, command[:2]))
        return clients.Proc(0, TOOLBOX if image == DEV_IMAGE else production, "")

    monkeypatch.setattr(clients, "run_container", run_container)
    monkeypatch.setattr(clients, "deployed_tag", lambda ctx, env: "v1.0.0")
    return ran


def test_a_fresh_toolbox_is_listed_in_plain_names(tmp_path, monkeypatch):
    ran = fake_images(monkeypatch)
    out = images.versions(make_ctx(tmp_path), images.ImageVersionsParams())
    assert ran == [(DEV_IMAGE, ["sh", "-c"])]
    assert out.data["reasons"][0] == {"label": "Rapportbouwer", "left": "versie 2.0.11"}
    assert out.data["reasons"][1] == {"label": "Databaseconnector", "left": "geïnstalleerd"}
    assert out.data["reasons"][3] == {"label": "Regelchecker", "left": "geïnstalleerd"}   # 0.0.0 is no real version
    assert [r["label"] for r in out.data["reasons"]] == [
        "Rapportbouwer", "Databaseconnector", "Layoutrobot", "Regelchecker", "Hulptool", "Checks voor het opslaan"]
    assert out.data["left_title"] == "Toolbox" and out.touched == set()
    assert "74194c7" in out.details and "dbt-bouncer" in out.details   # technical names and hashes: details only
    assert "74194c7" not in str(out.data["reasons"]) and "dbt" not in str(out.data["reasons"]).replace("Database", "")


def test_the_comparison_says_the_same_different_or_only_in_the_toolbox(tmp_path, monkeypatch):
    ran = fake_images(monkeypatch)
    out = images.versions(make_ctx(tmp_path), images.ImageVersionsParams(
        compare_with="production", expect_same=["report_builder", "connector"]))
    assert [i for i, _ in ran] == [DEV_IMAGE, "dbt-vdp:v1.0.0"]
    rows = out.data["reasons"]
    assert rows[0] == {"label": "Rapportbouwer", "left": "versie 2.0.11", "right": "versie 2.0.11", "same": True}
    assert rows[1] == {"label": "Databaseconnector", "left": "dezelfde build", "right": "dezelfde build", "same": True}
    assert rows[2] == {"label": "Layoutrobot", "left": "versie 0.30.0", "right": None, "same": False}
    assert (out.data["left_title"], out.data["right_title"]) == ("Toolbox", "Productiepackage")
    assert [t["state"] for t in out.data["tools"]][:3] == ["same", "same", "only_toolbox"]
    assert out.data["production_version"] == "v1.0.0" and isinstance(out.summary, str)


def test_a_different_version_fails_a_step_that_expects_the_same(tmp_path, monkeypatch):
    fake_images(monkeypatch, PRODUCTION.replace("dbt-oss 2.0.11", "dbt-oss 2.0.9"))
    with pytest.raises(StepFailed) as err:
        images.versions(make_ctx(tmp_path), images.ImageVersionsParams(compare_with="production", expect_same=["report_builder"]))
    assert err.value.summary == "Tussen beide is er een verschil bij: rapportbouwer."
    fake_images(monkeypatch, PRODUCTION.replace("dbt-oss 2.0.11", "dbt-oss 2.0.9"))
    out = images.versions(make_ctx(tmp_path), images.ImageVersionsParams(compare_with="production"))   # no expectation
    assert out.data["tools"][0]["state"] == "different"
    assert out.data["reasons"][0] == {"label": "Rapportbouwer", "left": "versie 2.0.11", "right": "versie 2.0.9", "same": False}


def test_a_container_that_cannot_start_is_a_plain_failure(tmp_path, monkeypatch):
    monkeypatch.setattr(clients, "run_container", lambda *a, **k: clients.Proc(125, "", "no such image"))
    with pytest.raises(StepFailed) as err:
        images.versions(make_ctx(tmp_path), images.ImageVersionsParams())
    assert err.value.summary == "Een verse container van het package kon niet geopend worden."
