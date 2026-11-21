import pydantic
import pytest

from studio.actions import clients, deploy, storage
from studio.actions.base import StepFailed
from test_actions_support import make_ctx

TAGS = ["v1.0.0", "v1.10.0", "v1.2.0", "v1.2.1", "2026-Q1.0", "2026-Q1.1", "2025-Q4.3", "latest", "<none>"]


def test_versions_sort_by_number_not_by_text():
    assert sorted(["v1.10.0", "v1.2.0", "v1.2.1"], key=deploy.version_key) == ["v1.2.0", "v1.2.1", "v1.10.0"]
    assert deploy.version_key("2026-Q1.1") > deploy.version_key("2025-Q4.3") > deploy.version_key("v9.9.9")
    assert deploy.version_key("latest") is None


def test_each_environment_has_its_own_family_of_versions():
    assert deploy.family("dev", TAGS, "v1.0.0") == ["v1.0.0", "v1.2.0", "v1.2.1", "v1.10.0"]
    assert deploy.family("prd", TAGS, "v1.0.0") == ["v1.0.0", "2025-Q4.3", "2026-Q1.0", "2026-Q1.1"]


@pytest.mark.parametrize(
    "spec,env,current,expected",
    [
        ("baseline", "prd", "2026-Q1.1", "v1.0.0"),
        ("latest", "prd", "2026-Q1.0", "2026-Q1.1"),
        ("latest", "dev", "v1.0.0", "v1.10.0"),
        ("previous", "prd", "2026-Q1.1", "2026-Q1.0"),
        ("previous", "dev", "v1.10.0", "v1.2.1"),
        ("previous", "prd", "2025-Q4.3", "v1.0.0"),
        ("previous", "prd", None, "v1.0.0"),
        ("v1.2.0", "dev", "v1.0.0", "v1.2.0"),
    ],
)
def test_pick_version(spec, env, current, expected):
    assert deploy.pick_version(spec, env, current, TAGS, "v1.0.0") == expected


def test_unbuilt_literal_and_empty_family_fail_plainly():
    with pytest.raises(StepFailed) as err:
        deploy.pick_version("v9.0.0", "dev", None, TAGS, "v1.0.0")
    assert "is nog niet gebuild" in err.value.summary
    with pytest.raises(StepFailed):
        deploy.pick_version("latest", "dev", None, ["2026-Q1.0"], "v1.0.0")


def test_params_accept_only_versions():
    for ok in ("baseline", "previous", "latest", "v1.2.3", "2026-Q1.0"):
        deploy.SetVersionParams(tag=ok)
    with pytest.raises(pydantic.ValidationError):
        deploy.SetVersionParams(tag="whatever")


def test_deploy_mirrors_the_ci_job_and_sets_the_version_last(tmp_path, monkeypatch):
    seen = []
    monkeypatch.setattr(clients, "docker", lambda ctx, *a, **k: seen.append(a) or clients.Proc(0, "\n".join(TAGS), ""))
    monkeypatch.setattr(
        clients,
        "airflow_cli",
        lambda ctx, env, *a, **k: seen.append(("airflow", env, *a)) or clients.Proc(0, "2026-Q1.0", ""),
    )
    ctx = make_ctx(tmp_path, "prd")
    result = deploy.set_version(ctx, deploy.SetVersionParams(tag="latest"))
    assert result.touched == {"version:prd"} and result.summary == "Productie runt nu versie 2026-Q1.1."
    runs = [a for a in seen if a[0] == "run"]
    assert len(runs) == 2 and "workshop_prd:/state" in runs[0] and "dbt-vdp:2026-Q1.1" in runs[0]
    assert seen[-1] == ("airflow", "prd", "variables", "set", "dbt_image_tag", "2026-Q1.1")


def test_cleaner_skips_a_deploy_when_the_baseline_already_runs(tmp_path, monkeypatch):
    seen, copies = [], []
    monkeypatch.setattr(clients, "airflow_cli", lambda ctx, env, *a, **k: clients.Proc(0, "v1.0.0", ""))
    monkeypatch.setattr(deploy, "deploy", lambda *a: seen.append(a))
    monkeypatch.setattr(storage, "restore_env_copy", lambda ctx, env: copies.append(env) or "ok")
    deploy.restore_version(make_ctx(tmp_path), "version:dev")
    assert seen == [] and copies == ["dev"]
    monkeypatch.setattr(clients, "airflow_cli", lambda ctx, env, *a, **k: clients.Proc(0, "v1.4.0", ""))
    deploy.restore_version(make_ctx(tmp_path), "version:dev")
    assert seen[0][1:] == ("dev", "v1.0.0") and copies == ["dev", "dev"]   # the copy in storage is put back in both cases
