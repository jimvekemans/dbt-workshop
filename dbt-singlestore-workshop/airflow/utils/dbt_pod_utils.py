"""Run dbt tasks where the environment runs them: Docker (the workshop stack) or OpenShift.

The DAGs only use the two decorators `dbt_task` and `python_task` (and `state_call`); DBT_RUNTIME (utils/dbt_config.py)
decides which runner is behind them:

- `DockerTaskRunner` (default): a throwaway container of the CI-built project image (dbt-vdp:<tag>). The Airflow
  container talks to the host Docker daemon, so the sibling container's mounts name host-side sources: the state
  volume (WORKSHOP_STATE_VOLUME, subpath mnt_dbt) is mounted at /mnt/dbt; the project itself is inside the image.
  Python tasks run in the Airflow container, which has the same volume mounted.
- `OpenShiftTaskRunner`: a pod per task (Kubernetes provider, `@task.kubernetes_cmd`), with the state PVC mounted at
  /mnt/dbt and the credentials taken from a secret. The Airflow worker has no state volume there, so python tasks
  run in a pod too.
"""

import base64
import functools
import hashlib
import inspect
import json
import logging
import os
import subprocess
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, Callable

try:  # Airflow 3 (Task SDK)
    from airflow.sdk import Variable, task
    from airflow.sdk.exceptions import AirflowException, AirflowSkipException

    _VARIABLE_DEFAULT = "default"
except ImportError:  # Airflow 2
    from airflow.decorators import task
    from airflow.exceptions import AirflowException, AirflowSkipException
    from airflow.models import Variable

    _VARIABLE_DEFAULT = "default_var"

from utils import dbt_state_utils
from utils.dbt_config import (
    DBT_IMAGE_REPOSITORY,
    DBT_IMAGE_TAG_VARIABLE,
    DEPLOY_TARGET,
    ENV,
    IMAGE_WORKDIR,
    MOUNT_DIR,
    OPENSHIFT_IMAGE,
    OPENSHIFT_KUBE_CONFIG,
    OPENSHIFT_NAMESPACE,
    OPENSHIFT_OPTIONAL_SECRET_KEYS,
    OPENSHIFT_PVC_CLAIM_NAME,
    OPENSHIFT_PVC_SUBPATH,
    OPENSHIFT_SECRET_NAME,
    OPENSHIFT_STATE_IMAGE,
    RUNTIME,
    SAFE_RUN_ID_TEMPLATE,
    SINGLESTORE_DB,
    STATE_VOLUME_SUBPATH,
    TARGET_DIR,
)

logger = logging.getLogger(__name__)

REQUIRED_CREDENTIALS = ("SINGLESTORE_HOST", "SINGLESTORE_USER", "SINGLESTORE_PASS")
OPTIONAL_PASSTHROUGH = ("IKL_HASH_SALT",)

# Where the Kubernetes provider's sidecar reads the XCom of a pod, and the exit code that marks a pod as skipped
XCOM_FILE = "/airflow/xcom/return.json"
SKIP_EXIT_CODE = 99


def get_variable(key: str, default: Any = None) -> Any:
    """Airflow Variable with a default."""
    return Variable.get(key, **{_VARIABLE_DEFAULT: default})


# ---------------------------------------------------------------------------
# What a task callable returns
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class StateCall:
    """A call of a function of utils/dbt_state_utils.py, to be made where the state volume is mounted."""

    func: str
    kwargs: dict[str, Any] = field(default_factory=dict)


def state_call(func: Callable, **kwargs: Any) -> StateCall:
    """What a `python_task` callable returns: `state_call(merge_run_results_and_manifest, source_dir=..., ...)`.

    Only functions of utils/dbt_state_utils.py with JSON-serialisable keyword arguments: on OpenShift the call is
    shipped to a pod, which knows nothing of the DAG file or the Airflow context.
    """
    name = getattr(func, "__name__", str(func))
    if getattr(dbt_state_utils, name, None) is not func:
        raise ValueError(f"state_call: {name!r} is not a function of utils/dbt_state_utils.py")
    json.dumps(kwargs)  # fails here, at the call site, rather than in the pod
    return StateCall(func=name, kwargs=kwargs)


def normalise_command(cmd: list[Any] | str) -> list[str]:
    """A dbt task callable returns a shell line or an argument list; runners work with the list."""
    if isinstance(cmd, str):
        return ["bash", "-c", cmd]
    if not isinstance(cmd, (list, tuple)) or not cmd:
        raise AirflowException(f"A dbt task must return a command (string or non-empty list), got {cmd!r}")
    return [str(part) for part in cmd]


def state_call_command(call: StateCall | None, do_xcom_push: bool = False, skip_if_missing: bool = False) -> list[str]:
    """The command that makes a state call in a container that only has python: the source of dbt_state_utils (it
    uses the standard library only) followed by the call. Base64, so neither the shell nor Jinja reads it.

    With `do_xcom_push` the result is written where the XCom sidecar reads it; with `skip_if_missing` a
    FileNotFoundError ends the pod with SKIP_EXIT_CODE instead of a failure."""
    if call is None:
        xcom = f" && mkdir -p {os.path.dirname(XCOM_FILE)} && echo null > {XCOM_FILE}" if do_xcom_push else ""
        return ["bash", "-c", f"echo 'Nothing to do on the dbt artifacts.'{xcom}"]
    script = "\n".join([
        inspect.getsource(dbt_state_utils),
        "import json, logging, os, sys",
        "logging.basicConfig(level=logging.INFO, format='%(levelname)s %(message)s')",
        "_result, _exit_code = None, 0",
        "try:",
        f"    _result = {call.func}(**json.loads({json.dumps(call.kwargs)!r}))",
        "except FileNotFoundError as _missing:",
        f"    if not {bool(skip_if_missing)}:",
        "        raise",
        "    print(f'Artifact missing, nothing to do: {_missing}')",
        f"    _exit_code = {SKIP_EXIT_CODE}",
        f"if {bool(do_xcom_push)}:",
        f"    os.makedirs(os.path.dirname({XCOM_FILE!r}), exist_ok=True)",
        f"    with open({XCOM_FILE!r}, 'w', encoding='utf-8') as _xcom:",
        "        json.dump(_result, _xcom)",
        "sys.exit(_exit_code)",
        "",
    ])
    payload = base64.b64encode(script.encode("utf-8")).decode("ascii")
    code = f"import base64; exec(base64.b64decode('{payload}').decode('utf-8'))"
    # python3 or python, whichever the image has; the code holds nothing a double-quoted shell string would expand
    return [
        "bash", "-c",
        "PY=$(command -v python3 || command -v python) || { echo 'This image has no python: a python_task cannot "
        f"run in it.' >&2; exit 127; }}; exec \"$PY\" -c \"{code}\"",
    ]


# ---------------------------------------------------------------------------
# Callable plumbing shared by the runners
# ---------------------------------------------------------------------------

_CONTEXT_SIGNATURE = inspect.Signature([inspect.Parameter("context", inspect.Parameter.VAR_KEYWORD)])


def _call_with_context(python_callable: Callable, context: dict[str, Any]) -> Any:
    """Call a task callable with the context arguments it declares (all of them for **kwargs / `context`)."""
    sig = inspect.signature(python_callable)
    accepts_kwargs = any(p.kind == inspect.Parameter.VAR_KEYWORD for p in sig.parameters.values())
    if accepts_kwargs or "context" in sig.parameters:
        return python_callable(**context)
    if len(sig.parameters) == 0:
        return python_callable()
    return python_callable(**{k: v for k, v in context.items() if k in sig.parameters})


def _context_callable(python_callable: Callable, body: Callable[[dict[str, Any]], Any]) -> Callable:
    """`body(context)` under the name and docstring of the task callable. Airflow passes the full context to a
    **kwargs callable, whatever the original one declared."""

    @functools.wraps(python_callable)
    def _wrapped(**context: Any) -> Any:
        return body(context)

    _wrapped.__signature__ = _CONTEXT_SIGNATURE
    return _wrapped


def _resolve_command(python_callable: Callable, context: dict[str, Any]) -> list[str]:
    """The dbt command of the task, with the Airflow Variable `dbt_env_vars` exported in front of it (read when the
    task runs) - see utils/dbt_runtime_overrides.py, when the project has that module."""
    cmd = _call_with_context(python_callable, context)
    try:
        from utils.dbt_runtime_overrides import apply_overrides, read_env_vars
    except ImportError:  # a project without the runtime overrides
        return normalise_command(cmd)
    env_vars = read_env_vars(get_variable)
    return normalise_command(apply_overrides(cmd, env_vars=env_vars) if env_vars else cmd)


def _resolve_state_call(python_callable: Callable, context: dict[str, Any]) -> StateCall | None:
    call = _call_with_context(python_callable, context)
    if call is not None and not isinstance(call, StateCall):
        raise AirflowException(
            f"python_task '{python_callable.__name__}' must return state_call(...) or None, got {type(call).__name__}"
        )
    return call


# ---------------------------------------------------------------------------
# Runners
# ---------------------------------------------------------------------------


class DbtTaskRunner(ABC):
    """Turns task callables into Airflow tasks. A runner decides where the command of a dbt task and the state call
    of a python task are executed; what the callables return is the same for every runner."""

    name: str

    def dbt_task(
        self,
        task_id: str,
        trigger_rule: str = "all_success",
        target_path: str = TARGET_DIR,
        extra_env_vars: dict[str, str] | None = None,
        **kwargs: Any,
    ) -> Callable:
        """Decorator for a callable returning a dbt command (shell line or argument list); the command runs in a
        container of the deployed project image."""
        env_vars = {
            "DBT_DEPLOY_TARGET": DEPLOY_TARGET,
            "LOG_RUN_RESULTS": "TRUE",
            "DBT_TARGET_PATH": target_path,
        }
        if extra_env_vars:
            env_vars.update(extra_env_vars)

        def _decorator(python_callable: Callable) -> Any:
            command = _context_callable(python_callable, lambda context: _resolve_command(python_callable, context))
            return self._command_task(task_id, command, env_vars, trigger_rule, **kwargs)

        return _decorator

    def python_task(
        self,
        task_id: str,
        trigger_rule: str = "all_done",
        do_xcom_push: bool = False,
        skip_if_missing: bool = False,
        **kwargs: Any,
    ) -> Callable:
        """Decorator for a callable that works on the dbt artifacts in /mnt/dbt. It gets the Airflow context and
        returns `state_call(<function of dbt_state_utils>, **kwargs)`, or None when there is nothing to do.

        `do_xcom_push`: what the function returns (JSON) becomes the XCom of the task.
        `skip_if_missing`: a FileNotFoundError of the function skips the task instead of failing it."""

        def _decorator(python_callable: Callable) -> Any:
            call = _context_callable(python_callable, lambda context: _resolve_state_call(python_callable, context))
            return self._state_task(task_id, call, trigger_rule, do_xcom_push, skip_if_missing, **kwargs)

        return _decorator

    @abstractmethod
    def _command_task(
        self, task_id: str, command: Callable[..., list[str]], env_vars: dict[str, str], trigger_rule: str,
        **kwargs: Any,
    ) -> Any:
        """The Airflow task that runs the command `command(**context)` returns, with `env_vars`, in a dbt container.
        Values of `env_vars` may contain SAFE_RUN_ID_TEMPLATE."""

    @abstractmethod
    def _state_task(
        self, task_id: str, call: Callable[..., StateCall | None], trigger_rule: str, do_xcom_push: bool,
        skip_if_missing: bool, **kwargs: Any,
    ) -> Any:
        """The Airflow task that makes the state call `call(**context)` returns where /mnt/dbt is mounted."""


def resolve_image() -> str:
    """dbt-vdp:<Variable dbt_image_tag>. The tag is set by the deploy job; unset means nothing was deployed yet."""
    tag = str(get_variable(DBT_IMAGE_TAG_VARIABLE, "") or "").strip()
    if not tag:
        raise AirflowException(
            f"Airflow Variable '{DBT_IMAGE_TAG_VARIABLE}' is not set: no dbt image was deployed to '{ENV}' yet. "
            f"Run the deploy job for a tag (it runs: airflow variables set {DBT_IMAGE_TAG_VARIABLE} <tag>)."
        )
    return f"{DBT_IMAGE_REPOSITORY}:{tag}"


class DockerTaskRunner(DbtTaskRunner):
    """Sibling containers of the Airflow container, on the workshop network."""

    name = "docker"

    @staticmethod
    def _state_mount() -> list[str]:
        """Only the mnt_dbt subpath of the environment's state volume (target/ and artifacts/) goes in."""
        volume = os.environ.get("WORKSHOP_STATE_VOLUME", "").strip()
        if not volume:
            raise AirflowException("WORKSHOP_STATE_VOLUME is not set: the dbt container has no state volume to mount.")
        return ["--mount", f"type=volume,src={volume},dst={MOUNT_DIR},volume-subpath={STATE_VOLUME_SUBPATH}"]

    @staticmethod
    def _connection_env() -> dict[str, str]:
        """Credentials come from this container's environment; nothing is defaulted."""
        missing = [key for key in REQUIRED_CREDENTIALS if not os.environ.get(key)]
        if missing:
            raise AirflowException(
                f"Missing environment variable(s) {', '.join(missing)} in the Airflow container: "
                "the dbt service user's connection details must be provided by the container environment."
            )
        env = {key: os.environ[key] for key in REQUIRED_CREDENTIALS}
        env.update({key: os.environ[key] for key in OPTIONAL_PASSTHROUGH if os.environ.get(key)})
        if SINGLESTORE_DB:
            env["SINGLESTORE_DB"] = SINGLESTORE_DB
        return env

    def run_container(self, task_id: str, cmd: list[str], env_vars: dict[str, str], run_id: str = "") -> None:
        """Run a command in a container of the deployed project image, attached to the workshop network."""
        safe_run_id = (run_id or "manual").replace(":", "_").replace("+", "_")
        resolved_env = {k: str(v).replace(SAFE_RUN_ID_TEMPLATE, safe_run_id) for k, v in env_vars.items()}
        resolved_env.update({k: v for k, v in self._connection_env().items() if k not in resolved_env})

        os.makedirs(os.path.join(MOUNT_DIR, "target"), exist_ok=True)
        os.makedirs(os.path.join(MOUNT_DIR, "artifacts"), exist_ok=True)

        image = resolve_image()
        network = os.environ.get("DOCKER_NETWORK", "workshop_net")
        logger.info(f"[Docker Executor] Task '{task_id}' runs image {image} (environment {ENV}, network {network})")

        # root + umask 0000: the state volume is shared with the Airflow container (uid differs), .dbtignore is
        # writable
        docker_cmd: list[str] = [
            "docker", "run", "--rm", "--user", "root", "--network", network, "-w", IMAGE_WORKDIR,
            *self._state_mount(),
        ]
        for k, v in resolved_env.items():
            docker_cmd.extend(["-e", f"{k}={v}"])

        parts = [p.replace(SAFE_RUN_ID_TEMPLATE, safe_run_id) for p in cmd]
        if len(parts) >= 3 and parts[0] == "bash" and parts[1] == "-c":
            exec_args = ["bash", "-c", f"umask 0000 && {parts[2]}"]
        else:
            exec_args = parts

        docker_cmd.append(image)
        docker_cmd.extend(exec_args)
        logger.info(f"[Docker Executor] Command: {' '.join(exec_args)}")

        proc_env = os.environ.copy()
        proc_env.setdefault("DOCKER_API_VERSION", "1.44")
        proc = subprocess.Popen(docker_cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=proc_env)
        output_lines: list[str] = []
        assert proc.stdout is not None
        for line in proc.stdout:
            stripped = line.rstrip("\n")
            output_lines.append(stripped)
            logger.info(stripped)

        return_code = proc.wait()
        if return_code != 0:
            tail_output = "\n".join(output_lines[-30:])
            raise AirflowException(
                f"Docker dbt container for task '{task_id}' ({image}) exited with code {return_code}.\n{tail_output}"
            )

    def _command_task(self, task_id, command, env_vars, trigger_rule, **kwargs):
        def _run(context: dict[str, Any]) -> None:
            self.run_container(task_id, command(**context), env_vars, run_id=context.get("run_id", ""))

        return task(task_id=task_id, trigger_rule=trigger_rule, **kwargs)(_context_callable(command, _run))

    def _state_task(self, task_id, call, trigger_rule, do_xcom_push, skip_if_missing, **kwargs):
        """The Airflow container has the state volume at /mnt/dbt: the call is made right here."""

        def _run(context: dict[str, Any]) -> Any:
            resolved = call(**context)
            if resolved is None:
                return None
            try:
                result = getattr(dbt_state_utils, resolved.func)(**resolved.kwargs)
            except FileNotFoundError as missing:
                if not skip_if_missing:
                    raise
                raise AirflowSkipException(f"Artifact missing, nothing to do: {missing}")
            return result if do_xcom_push else None

        return task(task_id=task_id, trigger_rule=trigger_rule, **kwargs)(_context_callable(call, _run))


class OpenShiftTaskRunner(DbtTaskRunner):
    """A pod per task in the environment's namespace (KubernetesPodOperator through `@task.kubernetes_cmd`).

    Needs apache-airflow-providers-cncf-kubernetes in the Airflow image; it is imported when a task is defined, so
    the Docker stack does not need it.
    """

    name = "openshift"

    @staticmethod
    def pod_name(task_id: str) -> str:
        candidate = f"openshift-{task_id.replace('_', '-').replace('.', '-').lower()}-pod"
        if len(candidate) <= 63:
            return candidate
        # A hash, not a cut: long task ids that share a prefix must not end up with the same pod name
        return f"dbt-{hashlib.md5(task_id.encode()).hexdigest()[:10]}-pod"

    def pod_template(self, task_id: str) -> dict[str, Any]:
        """The pod of a task: state PVC at /mnt/dbt, connection details from the secret. The image, the command and
        the plain environment variables are set by the operator on top of this."""
        volume_name = "dbt-pv-storage"
        secret_env = [
            {
                "name": key,
                "valueFrom": {
                    "secretKeyRef": {
                        "name": OPENSHIFT_SECRET_NAME,
                        "key": key,
                        **({"optional": True} if key in OPENSHIFT_OPTIONAL_SECRET_KEYS else {}),
                    }
                },
            }
            for key in (*REQUIRED_CREDENTIALS, *OPTIONAL_PASSTHROUGH)
        ]
        return {
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {"name": self.pod_name(task_id)},
            "spec": {
                "volumes": [{"name": volume_name, "persistentVolumeClaim": {"claimName": OPENSHIFT_PVC_CLAIM_NAME}}],
                "securityContext": {"runAsGroup": 0},
                "containers": [
                    {
                        "name": "dbt-container",
                        "imagePullPolicy": "Always",
                        "volumeMounts": [
                            {"mountPath": MOUNT_DIR, "name": volume_name, "subPath": OPENSHIFT_PVC_SUBPATH}
                        ],
                        "env": secret_env,
                    }
                ],
            },
        }

    def _pod_task(self, task_id, command, image, env_vars, trigger_rule, **kwargs):
        from airflow.providers.cncf.kubernetes.decorators.kubernetes_cmd import kubernetes_cmd_task

        return kubernetes_cmd_task(
            task_id=task_id,
            name=self.pod_name(task_id),  # the provider's default is derived from the function name (underscores)
            namespace=OPENSHIFT_NAMESPACE,
            config_file=OPENSHIFT_KUBE_CONFIG,
            image=image,
            pod_template_dict=self.pod_template(task_id),
            on_finish_action="delete_pod",
            trigger_rule=trigger_rule,
            env_vars=env_vars,  # a template field: SAFE_RUN_ID_TEMPLATE is rendered by Airflow
            **kwargs,
        )(command)

    def _command_task(self, task_id, command, env_vars, trigger_rule, **kwargs):
        if SINGLESTORE_DB:
            env_vars = {"SINGLESTORE_DB": SINGLESTORE_DB, **env_vars}
        return self._pod_task(task_id, command, OPENSHIFT_IMAGE, env_vars, trigger_rule, **kwargs)

    def _state_task(self, task_id, call, trigger_rule, do_xcom_push, skip_if_missing, **kwargs):
        def _command(context: dict[str, Any]) -> list[str]:
            resolved = call(**context)
            logger.info(f"[OpenShift Executor] State call of task '{task_id}': {resolved}")
            return state_call_command(resolved, do_xcom_push=do_xcom_push, skip_if_missing=skip_if_missing)

        if skip_if_missing:
            kwargs.setdefault("skip_on_exit_code", SKIP_EXIT_CODE)
        return self._pod_task(
            task_id, _context_callable(call, _command), OPENSHIFT_STATE_IMAGE, {"DBT_DEPLOY_TARGET": DEPLOY_TARGET},
            trigger_rule, do_xcom_push=do_xcom_push, **kwargs,
        )


RUNNERS: dict[str, type[DbtTaskRunner]] = {runner.name: runner for runner in (DockerTaskRunner, OpenShiftTaskRunner)}


def get_runner(runtime: str = RUNTIME) -> DbtTaskRunner:
    return RUNNERS[runtime]()


# What the DAGs import
runner = get_runner()
dbt_task = runner.dbt_task
python_task = runner.python_task
