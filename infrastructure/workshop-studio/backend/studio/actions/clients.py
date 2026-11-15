"""Shared connection helpers: SingleStore, the Airflow metadata database, the docker CLI and the GitLab REST API.

Nothing here knows about scenarios. Passwords are never hardcoded: SingleStore user passwords are parsed from the
CREATE USER statements in `settings.sql_dir`, the Airflow database password is read from the container environment of
`workshop-mysql`, the root password and the GitLab token come from the settings.
"""

from __future__ import annotations

import os
import re
import subprocess
import time
import urllib.parse
import uuid
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from typing import Any

import pymysql
import requests

from .base import Cancelled, Ctx, StepFailed

ENVS = ("dev", "val", "prd")
ENV_WORDS = {"dev": "development", "val": "validation", "prd": "production"}  # how the audience hears an environment
GITLAB_PROJECT = "workshop/dbt-vdp"
MYSQL_CONTAINER = "workshop-mysql"
CREATE_USER = re.compile(r"CREATE USER IF NOT EXISTS '([^']+)'@'%'\s+IDENTIFIED BY '([^']+)'", re.IGNORECASE)


# --- secrets --------------------------------------------------------------------------------------


@lru_cache(maxsize=4)
def _parse_passwords(sql_dir: str) -> dict[str, str]:
    """`{user: password}` from every CREATE USER in the SingleStore init files; `__ENV__` expands per environment."""
    found: dict[str, str] = {}
    for path in sorted(Path(sql_dir).glob("init_*")):
        for user, password in CREATE_USER.findall(path.read_text()):
            for name in ([user.replace("__ENV__", env) for env in ENVS] if "__ENV__" in user else [user]):
                found[name] = password
    return found


def password_for(settings: Any, user: str) -> str:
    if user == "root":
        return settings.s2_root_pwd
    passwords = _parse_passwords(str(settings.sql_dir))
    if user not in passwords:
        raise StepFailed(f"The stack has no database user called {user}.", f"known users: {sorted(passwords)}")
    return passwords[user]


def mask(settings: Any, text: str) -> str:
    """Replace every known secret in `text` by `***`, so technical details can be shown safely."""
    secrets = {settings.gitlab_pat, settings.s2_root_pwd, *_parse_passwords(str(settings.sql_dir)).values()}
    for secret in sorted((s for s in secrets if s), key=len, reverse=True):
        text = text.replace(secret, "***")
    return text


# --- SingleStore ----------------------------------------------------------------------------------


def s2_connect(settings: Any, user: str = "root", database: str | None = None, *, timeout: float = 5):
    return pymysql.connect(
        host=settings.s2_host,
        port=settings.s2_port,
        user=user,
        password=password_for(settings, user),
        database=database,
        autocommit=True,
        connect_timeout=timeout,
        read_timeout=timeout,
    )


def read_rows(settings: Any, query: str, *, limit: int = 200, timeout: float = 5, user: str = "root") -> tuple[list[str], list[tuple]]:
    """Run a read-only query as root: a single SELECT / WITH / SHOW, at most `limit` rows, `timeout` seconds."""
    text = query.strip().rstrip(";").strip()
    if not re.match(r"(?is)^(select|with|show)\b", text) or ";" in text:
        raise StepFailed("This view may only read data.", f"refused query: {text[:120]}")
    conn = s2_connect(settings, user, timeout=timeout)
    try:
        with conn.cursor() as cur:
            cur.execute(text)
            return [d[0] for d in cur.description or []], list(cur.fetchmany(limit))
    finally:
        conn.close()


# --- Airflow metadata database --------------------------------------------------------------------


@lru_cache(maxsize=1)
def _airflow_db_password() -> str:
    proc = subprocess.run(
        ["docker", "inspect", "-f", "{{range .Config.Env}}{{println .}}{{end}}", MYSQL_CONTAINER],
        capture_output=True,
        text=True,
        timeout=15,
    )
    for line in proc.stdout.splitlines():
        if line.startswith("MYSQL_PASSWORD="):
            return line.split("=", 1)[1]
    raise StepFailed(
        "The scheduler's database is not reachable.", f"no MYSQL_PASSWORD in {MYSQL_CONTAINER}: {proc.stderr}"
    )


def airflow_query(settings: Any, env: str, sql: str, args: tuple = ()) -> list[dict[str, Any]]:
    """Rows of a query against the metadata database `airflow_<env>` (user `airflow`)."""
    conn = pymysql.connect(
        host=settings.mysql_host,
        port=3306,
        user="airflow",
        password=_airflow_db_password(),
        database=f"airflow_{env}",
        connect_timeout=5,
        read_timeout=10,
        autocommit=True,
        cursorclass=pymysql.cursors.DictCursor,
    )
    try:
        with conn.cursor() as cur:
            cur.execute(sql, args)
            return list(cur.fetchall())
    finally:
        conn.close()


# --- docker and other commands --------------------------------------------------------------------


@dataclass
class Proc:
    returncode: int
    stdout: str
    stderr: str

    @property
    def text(self) -> str:
        return (self.stdout + self.stderr).strip()


def run(
    ctx: Ctx, argv: list[str], *, timeout: float = 600, cwd: str | None = None, env: dict[str, str] | None = None
) -> Proc:
    """Run a command. Output is masked; Cancel kills the process (Cancelled); `timeout` fails the step."""
    proc = subprocess.Popen(
        argv, cwd=cwd, env={**os.environ, **(env or {})}, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
    )
    deadline = time.monotonic() + timeout
    while True:
        try:
            out, err = proc.communicate(timeout=0.5)
            return Proc(proc.returncode, mask(ctx.settings, out), mask(ctx.settings, err))
        except subprocess.TimeoutExpired:
            if ctx.cancel.is_set() or time.monotonic() > deadline:
                proc.kill()
                proc.communicate()
                if ctx.cancel.is_set():
                    raise Cancelled() from None
                raise StepFailed(f"A step took longer than {int(timeout)} seconds and was stopped.", " ".join(argv[:4]))


def docker(ctx: Ctx, *args: str, timeout: float = 600, env: dict[str, str] | None = None, check: bool = True) -> Proc:
    proc = run(ctx, ["docker", *args], timeout=timeout, env=env)
    if check and proc.returncode != 0:
        raise StepFailed("The stack could not run a step it needs.", f"docker {' '.join(args[:3])}: {proc.text[-600:]}")
    return proc


WORKDIR = "/usr/app/dbt/workspace"


def docker_bytes(ctx: Ctx, *args: str, data: bytes | None = None, timeout: float = 300) -> bytes:
    """A docker command with binary stdin/stdout (`docker cp ... -` streams a tar archive)."""
    proc = subprocess.run(["docker", *args], input=data, capture_output=True, timeout=timeout)
    if proc.returncode != 0:
        raise StepFailed(
            "The stack could not copy files between containers.",
            mask(ctx.settings, proc.stderr.decode(errors="replace"))[-500:],
        )
    return proc.stdout


THROWAWAY_LABEL = "workshop.studio.throwaway"   # set on every container the studio starts for one step
THROWAWAY_NAME = re.compile(r"^studio-[0-9a-f]{8}$")


def throwaway_name() -> str:
    return f"studio-{uuid.uuid4().hex[:8]}"


def throwaway_flags(name: str) -> list[str]:
    return ["--name", name, "--label", f"{THROWAWAY_LABEL}=1"]


def remove_container(name: str) -> None:
    """`docker rm -f`, independent of Cancel (a cancelled step must still remove its container) and never raising."""
    try:
        subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=60)
    except (OSError, subprocess.SubprocessError):
        pass


def sweep_throwaway() -> list[str]:
    """Remove the containers an earlier run of the studio left behind (it was restarted or killed mid-step). Only
    containers with the studio's own name pattern AND label; nothing runs at start-up, so every one is stale."""
    try:
        listed = subprocess.run(
            ["docker", "ps", "-a", "--filter", f"label={THROWAWAY_LABEL}", "--format", "{{.Names}}"],
            capture_output=True, text=True, timeout=30,
        ).stdout.split()
    except (OSError, subprocess.SubprocessError):
        return []
    stale = [name for name in listed if THROWAWAY_NAME.match(name)]
    for name in stale:
        remove_container(name)
    return stale


def docker_run(ctx: Ctx, *args: str, timeout: float = 600, env: dict[str, str] | None = None, check: bool = True) -> Proc:
    """`docker run --rm <args>` in a named, labelled container that is removed afterwards even when the step is
    cancelled (killing the docker client does not stop the container)."""
    name = throwaway_name()
    try:
        return docker(ctx, "run", "--rm", *throwaway_flags(name), *args, timeout=timeout, env=env, check=check)
    finally:
        remove_container(name)


def project_tar(ctx: Ctx, image: str) -> bytes:
    """The dbt project (`workspace/...`, as a tar archive) exactly as it is inside `image`."""
    name = throwaway_name()
    try:
        docker(ctx, "create", *throwaway_flags(name), image)
        return docker_bytes(ctx, "cp", f"{name}:{WORKDIR}", "-")
    finally:
        remove_container(name)


def run_container(
    ctx: Ctx,
    image: str,
    command: list[str],
    *,
    env: dict[str, str] | None = None,
    secret_env: dict[str, str] | None = None,
    project: bytes | None = None,
    timeout: float = 1800,
) -> Proc:
    """Run `command` in a throwaway container of `image` on the workshop network; `project` (a `project_tar`) replaces
    the project inside the image. Secrets travel through the process environment, not through the command line.
    The container is removed afterwards, also when the presenter cancels or the step fails half way."""
    name = throwaway_name()
    flags = ["--network", ctx.settings.docker_network, "-w", WORKDIR]
    for key, value in (env or {}).items():
        flags += ["-e", f"{key}={value}"]
    for key in secret_env or {}:
        flags += ["-e", key]
    try:
        docker(ctx, "create", *throwaway_flags(name), *flags, image, *command, env=secret_env)
        if project is not None:
            docker_bytes(ctx, "cp", "-", f"{name}:/usr/app/dbt", data=project)
        return docker(ctx, "start", "-a", name, timeout=timeout, check=False)
    finally:
        remove_container(name)


def airflow_cli(ctx: Ctx, env: str, *args: str, timeout: float = 90, check: bool = True) -> Proc:
    """`docker exec workshop-airflow-<env> airflow ...`; the CLI's warning lines are dropped from stdout."""
    proc = docker(
        ctx, "exec", f"{ctx.settings.airflow_container_prefix}{env}", "airflow", *args, timeout=timeout, check=check
    )
    keep = [ln for ln in proc.stdout.splitlines() if " [warning " not in ln and " [info " not in ln]
    return Proc(proc.returncode, "\n".join(keep), proc.stderr)


def deployed_tag(ctx: Ctx, env: str) -> str:
    """The image tag the environment runs now (Airflow setting `dbt_image_tag`)."""
    proc = airflow_cli(ctx, env, "variables", "get", "dbt_image_tag", check=False)
    tag = proc.stdout.strip().splitlines()[-1] if proc.returncode == 0 and proc.stdout.strip() else ""
    if not tag:
        raise StepFailed(f"No version is deployed in {ENV_WORDS[env]} yet.", proc.text[-300:])
    return tag


# --- GitLab ---------------------------------------------------------------------------------------


def quote(text: str) -> str:
    return urllib.parse.quote(text, safe="")


class GitLabApi:
    """Small GitLab REST v4 client for project `workshop/dbt-vdp` with the root token."""

    def __init__(self, settings: Any):
        self.settings = settings
        self.base = f"{settings.gitlab_url}/api/v4"
        self.headers = {"PRIVATE-TOKEN": settings.gitlab_pat}
        self._pid: int | None = None

    def req(self, method: str, path: str, ok: tuple[int, ...] = (200, 201, 204), **kw) -> requests.Response:
        try:
            r = requests.request(method, f"{self.base}{path}", headers=self.headers, timeout=30, **kw)
        except requests.RequestException as exc:
            raise StepFailed("The code server is not reachable.", mask(self.settings, str(exc))) from exc
        if r.status_code not in ok:
            raise StepFailed(
                "The code server refused a request.",
                mask(self.settings, f"{method} {path} -> {r.status_code}: {r.text[:300]}"),
            )
        return r

    @property
    def pid(self) -> int:
        if self._pid is None:
            self._pid = self.req("GET", f"/projects/{quote(GITLAB_PROJECT)}").json()["id"]
        return self._pid

    def p(self, path: str) -> str:
        return f"/projects/{self.pid}{path}"

    def get(self, path: str, **params: Any) -> Any:
        return self.req("GET", self.p(path), params={"per_page": 100, **params}).json()

    def maybe(self, path: str) -> Any | None:
        r = self.req("GET", self.p(path), ok=(200, 404))
        return r.json() if r.status_code == 200 else None

    def branch_sha(self, name: str) -> str | None:
        found = self.maybe(f"/repository/branches/{quote(name)}")
        return found["commit"]["id"] if found else None

    def tag_sha(self, name: str) -> str | None:
        found = self.maybe(f"/repository/tags/{quote(name)}")
        return found["commit"]["id"] if found else None

    def branch_names(self) -> list[str]:
        return [b["name"] for b in self.get("/repository/branches")]

    def tag_names(self) -> list[str]:
        return [t["name"] for t in self.get("/repository/tags")]

    def read_file(self, path: str, ref: str) -> str | None:
        r = self.req("GET", self.p(f"/repository/files/{quote(path)}/raw"), ok=(200, 404), params={"ref": ref})
        return r.text if r.status_code == 200 else None

    def delete_branch(self, name: str) -> None:
        self.req("DELETE", self.p(f"/repository/branches/{quote(name)}"), ok=(200, 204, 404))

    def pipeline_for(self, sha: str, ref: str | None = None) -> dict[str, Any] | None:
        """Newest pipeline of a commit. Pass `ref`: a tag pipeline shares its commit with the branch pipeline."""
        params: dict[str, Any] = {"sha": sha, "order_by": "id", "sort": "desc", "per_page": 1}
        if ref:
            params["ref"] = ref
        found = self.req("GET", self.p("/pipelines"), params=params).json()
        return found[0] if found else None

    def jobs(self, pipeline_id: int) -> list[dict[str, Any]]:
        return self.get(f"/pipelines/{pipeline_id}/jobs")

    def failed_job_tail(self, pipeline_id: int, lines: int = 25) -> str:
        out = []
        for job in self.jobs(pipeline_id):
            if job["status"] == "failed":
                trace = self.req("GET", self.p(f"/jobs/{job['id']}/trace"), ok=(200, 404)).text
                out.append(f"--- {job['name']} (failed) ---\n" + "\n".join(trace.splitlines()[-lines:]))
        return mask(self.settings, "\n".join(out))
