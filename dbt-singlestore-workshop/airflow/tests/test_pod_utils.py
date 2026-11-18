"""Tests for the task runners (utils/dbt_pod_utils.py): Docker and OpenShift behind the same two decorators.

Run from airflow/:  python -m pytest tests   (no Airflow installation needed: the few Airflow names the module uses
are replaced by stand-ins that record how a task was declared and let the test run its callable).
"""
import json
import subprocess
import sys
import types
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


class _Declared:
    """What a task decorator received: its keyword arguments and the callable."""

    def __init__(self, kind, kwargs, python_callable):
        self.kind, self.kwargs, self.python_callable = kind, kwargs, python_callable

    def __call__(self):  # the DAG instantiates the task: `build_task = dbt_build()`
        return self


def _decorator(kind):
    def factory(**kwargs):
        return lambda python_callable: _Declared(kind, kwargs, python_callable)

    return factory


class _Variable:
    values: dict = {}

    @classmethod
    def get(cls, key, default=None):
        return cls.values.get(key, default)


def _install_airflow_stand_ins():
    class AirflowException(Exception):
        pass

    modules = {
        "airflow": types.ModuleType("airflow"),
        "airflow.sdk": types.ModuleType("airflow.sdk"),
        "airflow.sdk.exceptions": types.ModuleType("airflow.sdk.exceptions"),
        "airflow.providers": types.ModuleType("airflow.providers"),
        "airflow.providers.cncf": types.ModuleType("airflow.providers.cncf"),
        "airflow.providers.cncf.kubernetes": types.ModuleType("airflow.providers.cncf.kubernetes"),
        "airflow.providers.cncf.kubernetes.decorators": types.ModuleType("airflow.providers.cncf.kubernetes.decorators"),
        "airflow.providers.cncf.kubernetes.decorators.kubernetes_cmd": types.ModuleType("kubernetes_cmd"),
    }
    modules["airflow.sdk"].Variable = _Variable
    modules["airflow.sdk"].task = _decorator("task")
    modules["airflow.sdk.exceptions"].AirflowException = AirflowException
    modules["airflow.providers.cncf.kubernetes.decorators.kubernetes_cmd"].kubernetes_cmd_task = _decorator("pod")
    sys.modules.update(modules)


_install_airflow_stand_ins()

from utils import dbt_pod_utils as pod_utils  # noqa: E402
from utils.dbt_config import ENV, MOUNT_DIR, SAFE_RUN_ID_TEMPLATE, TARGET_DIR  # noqa: E402
from utils.dbt_pod_utils import (  # noqa: E402
    DbtTaskRunner, DockerTaskRunner, OpenShiftTaskRunner, StateCall, get_runner, normalise_command, state_call,
    state_call_command)
from utils.dbt_state_utils import consolidate_reload_artifacts, merge_run_results_and_manifest  # noqa: E402

CONTEXT = {"run_id": "manual__2026-11-18T10:00:00+00:00", "params": {}}


@pytest.fixture(autouse=True)
def _no_variables():
    _Variable.values = {}


def test_runtime_selects_the_runner_and_docker_is_the_default():
    assert isinstance(pod_utils.runner, DockerTaskRunner)
    assert isinstance(get_runner("docker"), DockerTaskRunner)
    assert isinstance(get_runner("openshift"), OpenShiftTaskRunner)
    with pytest.raises(TypeError):
        DbtTaskRunner()  # abstract


def test_normalise_command_accepts_a_shell_line_or_a_list():
    assert normalise_command("dbt build --target run") == ["bash", "-c", "dbt build --target run"]
    assert normalise_command(["dbt", "build", "--threads", 4]) == ["dbt", "build", "--threads", "4"]
    with pytest.raises(Exception):
        normalise_command([])


def test_state_call_only_takes_state_utils_functions_with_json_arguments():
    call = state_call(consolidate_reload_artifacts, target_path="/mnt/dbt/artifacts/reload/x")
    assert call == StateCall("consolidate_reload_artifacts", {"target_path": "/mnt/dbt/artifacts/reload/x"})
    with pytest.raises(ValueError):
        state_call(json.dumps, obj=1)
    with pytest.raises(TypeError):
        state_call(consolidate_reload_artifacts, target_path=object())


def _write_results(folder: Path, unique_id: str) -> None:
    folder.mkdir(parents=True)
    (folder / "run_results.json").write_text(json.dumps({"results": [{"unique_id": unique_id, "status": "success"}]}))


def test_state_call_command_runs_with_nothing_but_python(tmp_path):
    """The OpenShift pod has python3 and the PVC, not the DAG folder: the command must carry the code itself."""
    _write_results(tmp_path / "step_a", "model.a")
    _write_results(tmp_path / "step_b", "model.b")
    accumulated = tmp_path / "accumulated_state"
    for step in ("step_a", "step_b"):
        cmd = state_call_command(state_call(
            merge_run_results_and_manifest, source_dir=str(tmp_path / step), accumulated_state_dir=str(accumulated)))
        assert cmd[:2] == ["python3", "-c"] and "{{" not in cmd[2] and "{%" not in cmd[2]
        subprocess.run([sys.executable, "-I", "-c", cmd[2]], check=True, cwd=tmp_path)
    merged = json.loads((accumulated / "run_results.json").read_text())
    assert [r["unique_id"] for r in merged["results"]] == ["model.a", "model.b"]


def test_state_call_command_without_a_call_is_a_no_op():
    assert state_call_command(None)[:2] == ["bash", "-c"]


def test_openshift_dbt_task_declares_a_pod_and_returns_a_list_command():
    runner = OpenShiftTaskRunner()

    @runner.dbt_task(task_id="dbt_build", target_path=f"/mnt/dbt/x/{SAFE_RUN_ID_TEMPLATE}", retries=1)
    def dbt_build():
        """Docstring kept."""
        return "dbt build --target run"

    declared = dbt_build()
    assert declared.kind == "pod" and declared.python_callable.__doc__ == "Docstring kept."
    kwargs = declared.kwargs
    assert kwargs["task_id"] == "dbt_build" and kwargs["retries"] == 1 and kwargs["trigger_rule"] == "all_success"
    assert kwargs["on_finish_action"] == "delete_pod" and kwargs["name"] == "openshift-dbt-build-pod"
    assert kwargs["namespace"] == f"{ENV}-vdp-airflow"
    assert "dbt_image_tag" in kwargs["image"] and kwargs["image"].startswith("artifacts.vdab.be/")
    # the run id is left for Airflow to render: env_vars is a template field of the pod operator
    assert kwargs["env_vars"] == {
        "SINGLESTORE_DB": f"{ENV}_edw_dv_rdv", "DBT_DEPLOY_TARGET": ENV, "LOG_RUN_RESULTS": "TRUE",
        "DBT_TARGET_PATH": f"/mnt/dbt/x/{SAFE_RUN_ID_TEMPLATE}",
    }
    assert declared.python_callable(**CONTEXT) == ["bash", "-c", "dbt build --target run"]


def test_openshift_pod_template_mounts_the_pvc_and_reads_the_secret():
    template = OpenShiftTaskRunner().pod_template("dbt_build")
    container = template["spec"]["containers"][0]
    assert template["metadata"]["name"] == "openshift-dbt-build-pod"
    assert template["spec"]["volumes"][0]["persistentVolumeClaim"] == {"claimName": "pvc-vdp-airflow"}
    assert container["volumeMounts"] == [{"mountPath": MOUNT_DIR, "name": "dbt-pv-storage", "subPath": "dbt"}]
    env = {e["name"]: e["valueFrom"]["secretKeyRef"] for e in container["env"]}
    assert set(env) == {"SINGLESTORE_HOST", "SINGLESTORE_USER", "SINGLESTORE_PASS", "IKL_HASH_SALT"}
    assert env["SINGLESTORE_PASS"] == {"name": f"s-{ENV}-vdp-dbt", "key": "SINGLESTORE_PASS"}
    assert env["IKL_HASH_SALT"].get("optional") is True


def test_openshift_pod_names_stay_valid_and_distinct_for_long_task_ids():
    long_a, long_b = "merge_state__" + "a" * 60 + "_one", "merge_state__" + "a" * 60 + "_two"
    names = {OpenShiftTaskRunner.pod_name(long_a), OpenShiftTaskRunner.pod_name(long_b)}
    assert len(names) == 2 and all(len(n) <= 63 for n in names)
    assert OpenShiftTaskRunner.pod_name("batched_incremental_loads.batch_chunk_1") == \
        "openshift-batched-incremental-loads-batch-chunk-1-pod"


def test_openshift_python_task_ships_the_state_call_to_a_pod():
    runner = OpenShiftTaskRunner()

    @runner.python_task(task_id="merge_state__x", trigger_rule="all_success")
    def _merge(**context):
        if context["params"].get("dry_run"):
            return None
        return state_call(merge_run_results_and_manifest, source_dir=context["run_id"], accumulated_state_dir="/acc")

    declared = _merge()
    assert declared.kind == "pod" and declared.kwargs["trigger_rule"] == "all_success"
    assert declared.python_callable(**CONTEXT)[:2] == ["python3", "-c"]
    assert declared.python_callable(run_id="x", params={"dry_run": True})[:2] == ["bash", "-c"]


def test_docker_dbt_task_runs_the_command_in_a_container(monkeypatch):
    runner, ran = DockerTaskRunner(), []
    monkeypatch.setattr(runner, "run_container", lambda task_id, cmd, env_vars, run_id="": ran.append(
        (task_id, cmd, env_vars, run_id)))
    _Variable.values = {"dbt_env_vars": '{"DBT_SEVERITY_DEV_DATA": "warn"}'}

    @runner.dbt_task(task_id="dbt_build", extra_env_vars={"LOG_RUN_RESULTS": "FALSE"})
    def dbt_build():  # declares no context argument: the run id must reach the container all the same
        return ["dbt", "build"]

    declared = dbt_build()
    assert declared.kind == "task" and declared.kwargs == {"task_id": "dbt_build", "trigger_rule": "all_success"}
    declared.python_callable(**CONTEXT)
    task_id, cmd, env_vars, run_id = ran[0]
    assert (task_id, run_id) == ("dbt_build", CONTEXT["run_id"])
    assert cmd[:2] == ["bash", "-c"] and "DBT_SEVERITY_DEV_DATA" in cmd[2] and "dbt build" in cmd[2]
    assert env_vars == {"DBT_DEPLOY_TARGET": ENV, "LOG_RUN_RESULTS": "FALSE", "DBT_TARGET_PATH": TARGET_DIR}


def test_docker_python_task_makes_the_state_call_in_the_airflow_container(tmp_path):
    runner = DockerTaskRunner()
    _write_results(tmp_path / "init", "model.a")

    @runner.python_task(task_id="archive_reload_artifacts")
    def archive(**context):
        return state_call(consolidate_reload_artifacts, target_path=str(tmp_path))

    declared = archive()
    assert declared.kind == "task" and declared.kwargs["trigger_rule"] == "all_done"
    assert declared.python_callable(**CONTEXT) is None  # nothing large goes to XCom
    assert json.loads((tmp_path / "run_results.json").read_text())["results"][0]["unique_id"] == "model.a"


def test_python_task_rejects_anything_but_a_state_call():
    @DockerTaskRunner().python_task(task_id="wrong")
    def wrong():
        return {"status": "done"}

    with pytest.raises(Exception, match="state_call"):
        wrong().python_callable(**CONTEXT)
