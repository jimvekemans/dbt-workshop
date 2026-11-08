"""Docker and OpenShift/Kubernetes Pod execution utilities for dbt tasks."""

import functools
import hashlib
import inspect
import logging
import os
import subprocess
from typing import Any, Callable


from utils.compat import AirflowException, task
from utils.dbt_config import (
    ENV,
    ENV_UPPER,
    NAMESPACE,
    KUBE_CONFIG,
    MOUNT_DIR,
    TARGET_DIR,
    DBT_IMAGE,
    SECRET_NAME,
    PVC_CLAIM_NAME,
    DBT_PV_STORAGE_NAME,
    SAFE_RUN_ID_TEMPLATE,
)

logger = logging.getLogger(__name__)


def get_dbt_pod_template(task_id: str) -> dict[str, Any]:
    """Build OpenShift Kubernetes Pod specification for dbt execution."""
    candidate = f"openshift-{task_id.replace('_', '-')}-pod"
    if len(candidate) > 63:
        suffix = hashlib.md5(task_id.encode()).hexdigest()[:10]
        pod_name = f"dbt-{suffix}-pod"
        logger.warning(
            f"Task ID '{task_id}' generated pod name '{candidate}' ({len(candidate)} chars) "
            f"exceeding the 63-char Kubernetes limit; using hash-based name '{pod_name}'."
        )
    else:
        pod_name = candidate

    return {
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {"name": pod_name},
        "spec": {
            "volumes": [
                {
                    "name": DBT_PV_STORAGE_NAME,
                    "persistentVolumeClaim": {"claimName": PVC_CLAIM_NAME},
                }
            ],
            "securityContext": {"runAsGroup": 0},
            "containers": [
                {
                    "name": "dbt-container",
                    "image": DBT_IMAGE,
                    "imagePullPolicy": "Always",
                    "volumeMounts": [
                        {
                            "mountPath": MOUNT_DIR,
                            "name": DBT_PV_STORAGE_NAME,
                            "subPath": "dbt",
                        }
                    ],
                    "env": [
                        {
                            "name": key,
                            "valueFrom": {
                                "secretKeyRef": {
                                    "name": SECRET_NAME,
                                    "key": key,
                                }
                            },
                        }
                        for key in (
                            "SINGLESTORE_HOST",
                            "SINGLESTORE_USER",
                            "SINGLESTORE_PASS",
                            "IKL_HASH_SALT",
                        )
                    ],
                }
            ],
        },
    }


def _resolve_docker_image() -> str:
    """Resolve the dbt Docker image to execute, falling back to base image if needed."""
    for candidate in (
        DBT_IMAGE,
        "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-singlestore-base:1.11.11",
        "dbt-singlestore-base:1.11.11",
        "dbt-singlestore-base:latest",
    ):
        res = subprocess.run(
            ["docker", "image", "inspect", candidate],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if res.returncode == 0:
            return candidate
    return "dbt-singlestore-base:1.11.11"


def _workspace_mounts() -> list[str]:
    """The dbt workspace and the shared /mnt/dbt "PVC" of this environment, for a sibling container.

    The runner is started through the host's docker socket, so its mounts name host-side sources. In the workshop the
    LDV/PRD workspaces are Docker volumes (WORKSHOP_STATE_VOLUME=workshop_dev|workshop_prod, subdirectories dbt/ and
    mnt_dbt/ - the same subpaths this Airflow container mounts). HOST_WORKSHOP_STATE_DIR (host folder with <env>/dbt
    and <env>/mnt_dbt) remains as the fallback for a bind-mounted setup.
    """
    volume = os.environ.get("WORKSHOP_STATE_VOLUME", "").strip()
    if volume:
        return [
            "--mount", f"type=volume,src={volume},dst=/usr/app/dbt/workspace,volume-subpath=dbt",
            "--mount", f"type=volume,src={volume},dst={MOUNT_DIR},volume-subpath=mnt_dbt",
        ]
    host_state = os.environ.get("HOST_WORKSHOP_STATE_DIR", "").strip()
    if not host_state:
        raise AirflowException(
            "Neither WORKSHOP_STATE_VOLUME nor HOST_WORKSHOP_STATE_DIR is set: the dbt runner container has no workspace."
        )
    env_dir = os.environ.get("AIRFLOW_ENV_MODE", "dev" if ENV in ("ldv", "abt") else "prod")
    return [
        "-v", f"{os.path.join(host_state, env_dir, 'dbt')}:/usr/app/dbt/workspace",
        "-v", f"{os.path.join(host_state, env_dir, 'mnt_dbt')}:{MOUNT_DIR}",
    ]


def _execute_in_docker_container(
    task_id: str,
    cmd: list[str] | str,
    env_vars: dict[str, str],
    run_id: str = "",
) -> None:
    """Execute a dbt command inside an isolated Docker container attached to workshop_net."""
    safe_run_id = (run_id or "manual").replace(":", "_").replace("+", "_")
    resolved_env: dict[str, str] = {}
    for k, v in env_vars.items():
        val_str = str(v)
        if SAFE_RUN_ID_TEMPLATE in val_str:
            val_str = val_str.replace(SAFE_RUN_ID_TEMPLATE, safe_run_id)
        resolved_env[k] = val_str

    default_s2_user = "svc_ldv_dbt" if ENV in ("ldv", "abt") else "svc_prd_dbt"
    for secret_key, default_val in (
        ("SINGLESTORE_HOST", "singlestore"),
        ("SINGLESTORE_USER", default_s2_user),
        ("SINGLESTORE_PASS", "CiServicePwd2026!"),
        ("SINGLESTORE_DB", "edw_dv_rdv"),
        ("IKL_HASH_SALT", "workshop_salt_2026"),
    ):
        if secret_key not in resolved_env:
            resolved_env[secret_key] = os.environ.get(secret_key, default_val)

    os.makedirs(os.path.join(MOUNT_DIR, "target"), exist_ok=True)
    os.makedirs(os.path.join(MOUNT_DIR, "artifacts"), exist_ok=True)

    image = _resolve_docker_image()
    network = os.environ.get("DOCKER_NETWORK_NAME", "workshop_net")

    docker_cmd: list[str] = [
        "docker",
        "run",
        "--rm",
        "--user",
        "root",
        "--network",
        network,
        "-w",
        "/usr/app/dbt/workspace",
        *_workspace_mounts(),
    ]

    for k, v in resolved_env.items():
        docker_cmd.extend(["-e", f"{k}={v}"])

    if isinstance(cmd, str):
        exec_args = ["bash", "-c", f"umask 0000 && {cmd}"]
    else:
        resolved_parts = [
            (
                part.replace(SAFE_RUN_ID_TEMPLATE, safe_run_id)
                if isinstance(part, str)
                else str(part)
            )
            for part in cmd
        ]
        if len(resolved_parts) >= 3 and resolved_parts[0] == "bash" and resolved_parts[1] == "-c":
            exec_args = ["bash", "-c", f"umask 0000 && {resolved_parts[2]}"]
        else:
            exec_args = resolved_parts

    docker_cmd.append(image)
    docker_cmd.extend(exec_args)

    logger.info(
        f"[Docker Executor] Launching container '{image}' for task '{task_id}': {' '.join(exec_args)}"
    )

    proc_env = os.environ.copy()
    proc_env.setdefault("DOCKER_API_VERSION", "1.44")

    proc = subprocess.Popen(
        docker_cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env=proc_env,
    )
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
            f"Docker dbt container for task '{task_id}' exited with code {return_code}.\n{tail_output}"
        )


def _is_kubernetes_mode() -> bool:
    mode = os.environ.get("DBT_EXECUTION_MODE", "docker").lower()
    return mode == "kubernetes" and os.path.exists(KUBE_CONFIG)


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
    task runs, in docker and Kubernetes mode alike) - see utils/dbt_runtime_overrides.py."""
    from utils.compat import get_variable
    from utils.dbt_runtime_overrides import apply_overrides, read_env_vars

    @functools.wraps(python_callable)
    def _cmd(**context: Any) -> Any:
        cmd = _call_with_context(python_callable, context)
        env_vars = read_env_vars(get_variable)
        return apply_overrides(cmd, env_vars=env_vars) if env_vars else cmd

    _cmd.__signature__ = inspect.Signature(   # Airflow passes the full context to a **kwargs callable
        [inspect.Parameter("context", inspect.Parameter.VAR_KEYWORD)])
    return _cmd


def openshift_dbt_task(
    task_id: str,
    trigger_rule: str = "all_success",
    target_path: str = TARGET_DIR,
    extra_env_vars: dict[str, str] | None = None,
    **kwargs: Any,
) -> Callable:
    """Wrapper for dbt CLI commands executing via Docker executor or OpenShift Kubernetes pod."""
    env_vars = {
        "DBT_DEPLOY_TARGET": ENV_UPPER,
        "LOG_RUN_RESULTS": "TRUE",
        "DBT_TARGET_PATH": target_path,
    }
    if extra_env_vars:
        env_vars.update(extra_env_vars)

    if _is_kubernetes_mode():
        from airflow.providers.cncf.kubernetes.decorators.kubernetes_cmd import (
            kubernetes_cmd_task,
        )

        k8s_decorator = kubernetes_cmd_task(
            task_id=task_id,
            namespace=NAMESPACE,
            config_file=KUBE_CONFIG,
            pod_template_dict=get_dbt_pod_template(task_id),
            on_finish_action="delete_pod",
            trigger_rule=trigger_rule,
            env_vars=env_vars,
            **kwargs,
        )
        return lambda python_callable: k8s_decorator(_with_runtime_env(python_callable))

    def _decorator(python_callable: Callable) -> Callable:
        runtime_cmd = _with_runtime_env(python_callable)

        @functools.wraps(python_callable)
        def _wrapped(**context: Any) -> Any:
            cmd = runtime_cmd(**context)
            run_id = context.get("run_id", "")
            return _execute_in_docker_container(
                task_id=task_id,
                cmd=cmd,
                env_vars=env_vars,
                run_id=run_id,
            )

        return task(
            task_id=task_id,
            trigger_rule=trigger_rule,
            **kwargs,
        )(_wrapped)

    return _decorator


def openshift_python_task(
    task_id: str,
    trigger_rule: str = "all_done",
    extra_env_vars: dict[str, str] | None = None,
    **kwargs: Any,
) -> Callable:
    """Wrapper for Python tasks interacting with dbt artifacts on shared storage."""
    env_vars = {"DBT_DEPLOY_TARGET": ENV_UPPER}
    if extra_env_vars:
        env_vars.update(extra_env_vars)

    if _is_kubernetes_mode():
        return task.kubernetes(
            task_id=task_id,
            namespace=NAMESPACE,
            config_file=KUBE_CONFIG,
            pod_template_dict=get_dbt_pod_template(task_id),
            on_finish_action="delete_pod",
            trigger_rule=trigger_rule,
            env_vars=env_vars,
            **kwargs,
        )

    def _decorator(python_callable: Callable) -> Callable:
        @functools.wraps(python_callable)
        def _wrapped(**context: Any) -> Any:
            for k, v in env_vars.items():
                os.environ[k] = str(v)
            sig = inspect.signature(python_callable)
            accepts_kwargs = any(
                p.kind == inspect.Parameter.VAR_KEYWORD
                for p in sig.parameters.values()
            )
            if accepts_kwargs or "context" in sig.parameters:
                return python_callable(**context)
            elif len(sig.parameters) == 0:
                return python_callable()
            else:
                filtered = {
                    k: v for k, v in context.items() if k in sig.parameters
                }
                return python_callable(**filtered)

        return task(
            task_id=task_id,
            trigger_rule=trigger_rule,
            **kwargs,
        )(_wrapped)

    return _decorator
