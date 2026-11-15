"""Run dbt tasks in throwaway Docker containers of the CI-built project image (dbt-vdp:<tag>).

The Airflow container talks to the host Docker daemon, so the sibling container's mounts name host-side sources: the
state volume (WORKSHOP_STATE_VOLUME, subpath mnt_dbt) is mounted at /mnt/dbt; the project itself is inside the image.
"""

import functools
import inspect
import logging
import os
import subprocess
from typing import Any, Callable

from airflow.sdk import Variable, task
from airflow.sdk.exceptions import AirflowException

from utils.dbt_config import (
    DBT_IMAGE_REPOSITORY,
    DBT_IMAGE_TAG_VARIABLE,
    ENV,
    IMAGE_WORKDIR,
    MOUNT_DIR,
    SAFE_RUN_ID_TEMPLATE,
    SINGLESTORE_DB,
    STATE_VOLUME_SUBPATH,
    TARGET_DIR,
)

logger = logging.getLogger(__name__)

REQUIRED_CREDENTIALS = ("SINGLESTORE_HOST", "SINGLESTORE_USER", "SINGLESTORE_PASS")
OPTIONAL_PASSTHROUGH = ("IKL_HASH_SALT",)


def get_variable(key: str, default: Any = None) -> Any:
    """Airflow Variable with a default."""
    return Variable.get(key, default=default)


def resolve_image() -> str:
    """dbt-vdp:<Variable dbt_image_tag>. The tag is set by the deploy job; unset means nothing was deployed yet."""
    tag = str(get_variable(DBT_IMAGE_TAG_VARIABLE, "") or "").strip()
    if not tag:
        raise AirflowException(
            f"Airflow Variable '{DBT_IMAGE_TAG_VARIABLE}' is not set: no dbt image was deployed to '{ENV}' yet. "
            f"Run the deploy job for a tag (it runs: airflow variables set {DBT_IMAGE_TAG_VARIABLE} <tag>)."
        )
    return f"{DBT_IMAGE_REPOSITORY}:{tag}"


def _state_mount() -> list[str]:
    """Only the mnt_dbt subpath of the environment's state volume (target/ and artifacts/) goes into the container."""
    volume = os.environ.get("WORKSHOP_STATE_VOLUME", "").strip()
    if not volume:
        raise AirflowException("WORKSHOP_STATE_VOLUME is not set: the dbt container has no state volume to mount.")
    return ["--mount", f"type=volume,src={volume},dst={MOUNT_DIR},volume-subpath={STATE_VOLUME_SUBPATH}"]


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
    env["SINGLESTORE_DB"] = SINGLESTORE_DB
    return env


def _execute_in_docker_container(
    task_id: str,
    cmd: list[str] | str,
    env_vars: dict[str, str],
    run_id: str = "",
) -> None:
    """Run a dbt command in a container of the deployed project image, attached to the workshop network."""
    safe_run_id = (run_id or "manual").replace(":", "_").replace("+", "_")
    resolved_env = {k: str(v).replace(SAFE_RUN_ID_TEMPLATE, safe_run_id) for k, v in env_vars.items()}
    resolved_env.update({k: v for k, v in _connection_env().items() if k not in resolved_env})

    os.makedirs(os.path.join(MOUNT_DIR, "target"), exist_ok=True)
    os.makedirs(os.path.join(MOUNT_DIR, "artifacts"), exist_ok=True)

    image = resolve_image()
    network = os.environ.get("DOCKER_NETWORK", "workshop_net")
    logger.info(f"[Docker Executor] Task '{task_id}' runs image {image} (environment {ENV}, network {network})")

    # root + umask 0000: the state volume is shared with the Airflow container (uid differs), .dbtignore is writable
    docker_cmd: list[str] = [
        "docker", "run", "--rm", "--user", "root", "--network", network, "-w", IMAGE_WORKDIR, *_state_mount(),
    ]
    for k, v in resolved_env.items():
        docker_cmd.extend(["-e", f"{k}={v}"])

    if isinstance(cmd, str):
        exec_args = ["bash", "-c", f"umask 0000 && {cmd}"]
    else:
        parts = [p.replace(SAFE_RUN_ID_TEMPLATE, safe_run_id) if isinstance(p, str) else str(p) for p in cmd]
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


def _call_with_context(python_callable: Callable, context: dict[str, Any]) -> Any:
    """Call a task callable with the context arguments it declares (all of them for **kwargs / `context`)."""
    sig = inspect.signature(python_callable)
    accepts_kwargs = any(p.kind == inspect.Parameter.VAR_KEYWORD for p in sig.parameters.values())
    if accepts_kwargs or "context" in sig.parameters:
        return python_callable(**context)
    if len(sig.parameters) == 0:
        return python_callable()
    return python_callable(**{k: v for k, v in context.items() if k in sig.parameters})


def _with_runtime_env(python_callable: Callable) -> Callable:
    """The dbt command of the task, with the Airflow Variable `dbt_env_vars` exported in front of it (read when the
    task runs) - see utils/dbt_runtime_overrides.py."""
    from utils.dbt_runtime_overrides import apply_overrides, read_env_vars

    @functools.wraps(python_callable)
    def _cmd(**context: Any) -> Any:
        cmd = _call_with_context(python_callable, context)
        env_vars = read_env_vars(get_variable)
        return apply_overrides(cmd, env_vars=env_vars) if env_vars else cmd

    _cmd.__signature__ = inspect.Signature(   # Airflow passes the full context to a **kwargs callable
        [inspect.Parameter("context", inspect.Parameter.VAR_KEYWORD)])
    return _cmd


def dbt_task(
    task_id: str,
    trigger_rule: str = "all_success",
    target_path: str = TARGET_DIR,
    extra_env_vars: dict[str, str] | None = None,
    **kwargs: Any,
) -> Callable:
    """Decorator for a callable returning a dbt command; the command runs in a container of the deployed image."""
    env_vars = {
        "DBT_DEPLOY_TARGET": ENV,
        "LOG_RUN_RESULTS": "TRUE",
        "DBT_TARGET_PATH": target_path,
    }
    if extra_env_vars:
        env_vars.update(extra_env_vars)

    def _decorator(python_callable: Callable) -> Callable:
        runtime_cmd = _with_runtime_env(python_callable)

        @functools.wraps(python_callable)
        def _wrapped(**context: Any) -> Any:
            cmd = runtime_cmd(**context)
            return _execute_in_docker_container(
                task_id=task_id, cmd=cmd, env_vars=env_vars, run_id=context.get("run_id", ""),
            )

        return task(task_id=task_id, trigger_rule=trigger_rule, **kwargs)(_wrapped)

    return _decorator


def python_task(
    task_id: str,
    trigger_rule: str = "all_done",
    extra_env_vars: dict[str, str] | None = None,
    **kwargs: Any,
) -> Callable:
    """Decorator for Python tasks that work on the dbt artifacts in /mnt/dbt (run in the Airflow container)."""
    env_vars = {"DBT_DEPLOY_TARGET": ENV}
    if extra_env_vars:
        env_vars.update(extra_env_vars)

    def _decorator(python_callable: Callable) -> Callable:
        @functools.wraps(python_callable)
        def _wrapped(**context: Any) -> Any:
            for k, v in env_vars.items():
                os.environ[k] = str(v)
            return _call_with_context(python_callable, context)

        return task(task_id=task_id, trigger_rule=trigger_rule, **kwargs)(_wrapped)

    return _decorator
