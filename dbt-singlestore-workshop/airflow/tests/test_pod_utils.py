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

    class AirflowSkipException(AirflowException):
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
    modules["airflow.sdk.exceptions"].AirflowSkipException = AirflowSkipException
    modules["airflow.providers.cncf.kubernetes.decorators.kubernetes_cmd"].kubernetes_cmd_task = _decorator("pod")
    sys.modules.update(modules)


_install_airflow_stand_ins()

from utils import dbt_pod_utils as pod_utils  # noqa: E402
from utils import dbt_config, dbt_state_utils  # noqa: E402
from utils.dbt_config import DEPLOY_TARGET, MOUNT_DIR, SAFE_RUN_ID_TEMPLATE, TARGET_DIR  # noqa: E402
from utils.dbt_pod_utils import (  # noqa: E402
    DbtTaskRunner, DockerTaskRunner, OpenShiftTaskRunner, StateCall, get_runner, normalise_command, state_call,
    state_call_command)
from utils.dbt_state_utils import merge_run_results_and_manifest  # noqa: E402

SkipException = sys.modules["airflow.sdk.exceptions"].AirflowSkipException

CONTEXT = {"run_id": "manual__2026-11-18T10:00:00+00:00", "params": {}}


@pytest.fixture(autouse=True)
def _no_variables():
    _Variable.values = {}


def test_runtime_selects_the_runner():
    assert pod_utils.runner.name == dbt_config.RUNTIME
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
    call = state_call(merge_run_results_and_manifest, source_dir="/mnt/dbt/a", accumulated_state_dir="/mnt/dbt/b")
    assert call == StateCall(
        "merge_run_results_and_manifest", {"source_dir": "/mnt/dbt/a", "accumulated_state_dir": "/mnt/dbt/b"})
    with pytest.raises(ValueError):
        state_call(json.dumps, obj=1)
    with pytest.raises(TypeError):
        state_call(merge_run_results_and_manifest, source_dir=object())


def _write_results(folder: Path, unique_id: str) -> None:
    folder.mkdir(parents=True)
    (folder / "run_results.json").write_text(json.dumps({"results": [{"unique_id": unique_id, "status": "success"}]}))


def _run_pod_command(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess:
    """Run a state call command the way the pod does: bash, and no access to this project's modules."""
    assert cmd[:2] == ["bash", "-c"] and "{{" not in cmd[2] and "{%" not in cmd[2]
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env={"PATH": "/usr/bin:/bin"})


def test_state_call_command_runs_with_nothing_but_python(tmp_path):
    """The OpenShift pod has python and the PVC, not the DAG folder: the command must carry the code itself."""
    _write_results(tmp_path / "step_a", "model.a")
    _write_results(tmp_path / "step_b", "model.b")
    accumulated = tmp_path / "accumulated_state"
    for step in ("step_a", "step_b"):
        done = _run_pod_command(state_call_command(state_call(
            merge_run_results_and_manifest, source_dir=str(tmp_path / step), accumulated_state_dir=str(accumulated)
        )), tmp_path)
        assert done.returncode == 0, done.stderr
    merged = json.loads((accumulated / "run_results.json").read_text())
    assert [r["unique_id"] for r in merged["results"]] == ["model.a", "model.b"]


def _missing_artifact(path: str) -> dict:
    raise FileNotFoundError(f"Missing file: {path}")


def test_state_call_command_writes_the_xcom_and_skips_on_a_missing_artifact(tmp_path, monkeypatch):
    xcom_file = tmp_path / "xcom" / "return.json"
    monkeypatch.setattr(pod_utils, "XCOM_FILE", str(xcom_file))
    _write_results(tmp_path / "step_a", "model.a")

    call = state_call(merge_run_results_and_manifest, source_dir=str(tmp_path / "step_a"),
                      accumulated_state_dir=str(tmp_path / "acc"))
    assert _run_pod_command(state_call_command(call, do_xcom_push=True), tmp_path).returncode == 0
    assert json.loads(xcom_file.read_text())["results"][0]["unique_id"] == "model.a"

    # a function of dbt_state_utils that does not find its artifact
    monkeypatch.setattr(dbt_state_utils, "_missing_artifact", _missing_artifact, raising=False)
    monkeypatch.setattr(pod_utils.inspect, "getsource", lambda module: "import json, logging, os\n" + (
        "def _missing_artifact(path):\n    raise FileNotFoundError(path)\n"))
    missing = StateCall("_missing_artifact", {"path": "/nowhere/run_results.json"})
    skipped = _run_pod_command(state_call_command(missing, do_xcom_push=True, skip_if_missing=True), tmp_path)
    assert skipped.returncode == pod_utils.SKIP_EXIT_CODE and "/nowhere/run_results.json" in skipped.stdout
    assert json.loads(xcom_file.read_text()) is None  # the sidecar always finds a file
    assert _run_pod_command(state_call_command(missing), tmp_path).returncode == 1


def test_state_call_command_without_a_call_is_a_no_op(tmp_path, monkeypatch):
    xcom_file = tmp_path / "xcom" / "return.json"
    monkeypatch.setattr(pod_utils, "XCOM_FILE", str(xcom_file))
    assert _run_pod_command(state_call_command(None), tmp_path).returncode == 0 and not xcom_file.exists()
    assert _run_pod_command(state_call_command(None, do_xcom_push=True), tmp_path).returncode == 0
    assert json.loads(xcom_file.read_text()) is None


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
    assert kwargs["namespace"] == dbt_config.OPENSHIFT_NAMESPACE and kwargs["namespace"].endswith("-vdp-airflow")
    assert kwargs["config_file"] == dbt_config.OPENSHIFT_KUBE_CONFIG
    assert kwargs["image"] == dbt_config.OPENSHIFT_IMAGE and kwargs["image"].startswith("artifacts.vdab.be/")
    # the run id is left for Airflow to render: env_vars is a template field of the pod operator
    database = {"SINGLESTORE_DB": dbt_config.SINGLESTORE_DB} if dbt_config.SINGLESTORE_DB else {}
    assert kwargs["env_vars"] == {
        **database, "DBT_DEPLOY_TARGET": DEPLOY_TARGET, "LOG_RUN_RESULTS": "TRUE",
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
    assert env["SINGLESTORE_PASS"] == {"name": dbt_config.OPENSHIFT_SECRET_NAME, "key": "SINGLESTORE_PASS"}
    assert dbt_config.OPENSHIFT_SECRET_NAME.startswith("s-") and dbt_config.OPENSHIFT_SECRET_NAME.endswith("-vdp-dbt")
    optional = {key for key, ref in env.items() if ref.get("optional")}
    assert optional == set(dbt_config.OPENSHIFT_OPTIONAL_SECRET_KEYS)


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
    assert declared.kwargs["do_xcom_push"] is False and "skip_on_exit_code" not in declared.kwargs
    assert declared.kwargs["env_vars"] == {"DBT_DEPLOY_TARGET": DEPLOY_TARGET}
    assert "exec" in declared.python_callable(**CONTEXT)[2]
    assert "Nothing to do" in declared.python_callable(run_id="x", params={"dry_run": True})[2]

    @runner.python_task(task_id="extract", do_xcom_push=True, skip_if_missing=True)
    def _extract():
        return state_call(merge_run_results_and_manifest, source_dir="/a", accumulated_state_dir="/b")

    kwargs = _extract().kwargs
    assert kwargs["do_xcom_push"] is True and kwargs["skip_on_exit_code"] == pod_utils.SKIP_EXIT_CODE


def test_docker_dbt_task_runs_the_command_in_a_container(monkeypatch):
    runner, ran = DockerTaskRunner(), []
    monkeypatch.setattr(runner, "run_container", lambda task_id, cmd, env_vars, run_id="": ran.append(
        (task_id, cmd, env_vars, run_id)))

    @runner.dbt_task(task_id="dbt_build", extra_env_vars={"LOG_RUN_RESULTS": "FALSE"})
    def dbt_build():  # declares no context argument: the run id must reach the container all the same
        return ["dbt", "build"]

    declared = dbt_build()
    assert declared.kind == "task" and declared.kwargs == {"task_id": "dbt_build", "trigger_rule": "all_success"}
    declared.python_callable(**CONTEXT)
    task_id, cmd, env_vars, run_id = ran[0]
    assert (task_id, run_id) == ("dbt_build", CONTEXT["run_id"])
    assert cmd == ["dbt", "build"]
    assert env_vars == {"DBT_DEPLOY_TARGET": DEPLOY_TARGET, "LOG_RUN_RESULTS": "FALSE", "DBT_TARGET_PATH": TARGET_DIR}


def test_dbt_env_vars_variable_is_exported_in_front_of_the_command(monkeypatch):
    pytest.importorskip("utils.dbt_runtime_overrides")
    runner, ran = DockerTaskRunner(), []
    monkeypatch.setattr(runner, "run_container", lambda task_id, cmd, env_vars, run_id="": ran.append(cmd))
    _Variable.values = {"dbt_env_vars": '{"DBT_SEVERITY_DEV_DATA": "warn"}'}

    @runner.dbt_task(task_id="dbt_build")
    def dbt_build():
        return ["dbt", "build"]

    dbt_build().python_callable(**CONTEXT)
    assert ran[0][:2] == ["bash", "-c"] and "DBT_SEVERITY_DEV_DATA" in ran[0][2] and "dbt build" in ran[0][2]


def test_docker_python_task_makes_the_state_call_in_the_airflow_container(tmp_path, monkeypatch):
    runner = DockerTaskRunner()
    _write_results(tmp_path / "init", "model.a")

    def merge(do_xcom_push):
        @runner.python_task(task_id="merge_state", do_xcom_push=do_xcom_push)
        def _merge(**context):
            return state_call(merge_run_results_and_manifest, source_dir=str(tmp_path / "init"),
                              accumulated_state_dir=str(tmp_path / "acc"))

        return _merge()

    declared = merge(False)
    assert declared.kind == "task" and declared.kwargs == {"task_id": "merge_state", "trigger_rule": "all_done"}
    assert declared.python_callable(**CONTEXT) is None  # nothing goes to XCom unless asked
    assert json.loads((tmp_path / "acc" / "run_results.json").read_text())["results"][0]["unique_id"] == "model.a"
    assert merge(True).python_callable(**CONTEXT)["results"][0]["unique_id"] == "model.a"

    monkeypatch.setattr(dbt_state_utils, "_missing_artifact", _missing_artifact, raising=False)
    for skip_if_missing, expected in ((True, SkipException), (False, FileNotFoundError)):
        @runner.python_task(task_id="extract", skip_if_missing=skip_if_missing)
        def _extract():
            return StateCall("_missing_artifact", {"path": "/nowhere"})

        with pytest.raises(expected):
            _extract().python_callable(**CONTEXT)


def test_python_task_rejects_anything_but_a_state_call():
    @DockerTaskRunner().python_task(task_id="wrong")
    def wrong():
        return {"status": "done"}

    with pytest.raises(Exception, match="state_call"):
        wrong().python_callable(**CONTEXT)
