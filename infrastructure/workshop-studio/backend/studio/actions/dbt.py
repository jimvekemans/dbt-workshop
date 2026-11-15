"""`dbt.run`: run a dbt command in a throwaway container, as the environment's robot, the CI robot or a developer.

* `service`        image `dbt-vdp:<tag deployed in the environment>`, user `svc_<env>_dbt`, `--target run`
* `ci`             the same image of dev, user `svc_ci_dbt`, `--target ci --vars 'SCHEMA: demo'` (database `vdp_ci`)
* `developer:<u>`  the developer image `dbt-singlestore-dev:1.11.11` with the project of the deployed image copied in,
                   user `<u>`, profile target `default`, so that tables get the `<u>_` prefix

Cleaners: `models:<env>` rebuilds the environment's tables from scratch with the deployed image, `developer:<env>:<u>`
drops a developer's prefixed copies, `ci` drops what the sandbox run left in schema `demo`.
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from typing import Literal

from pydantic import ConfigDict, Field, field_validator

from .. import names
from . import clients
from .base import Ctx, Params, Result, StepFailed, action, cleaner

DEV_IMAGE = "dbt-singlestore-dev:1.11.11"
FINISHED = re.compile(r"Finished '([\w-]+)'(.*)")
CI_VARS = "SCHEMA: demo"
PLAIN_COMMANDS = {
    "build": "build of the reports",
    "run": "build of the reports",
    "test": "data checks",
    "seed": "loading of reference data",
    "snapshot": "history snapshot",
    "run-operation": "maintenance step",
    "source": "check of the data feeds",
    "compile": "check of the code",
    "parse": "check of the code",
    "retry": "retry of the failed part",
}


class DbtRunParams(Params):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    as_: str = Field("service", alias="as")
    command: str
    expect: Literal["success", "failed"] = "success"
    timeout: int = 1800

    @field_validator("as_")
    @classmethod
    def known_identity(cls, who: str) -> str:
        if who not in ("service", "ci") and not re.fullmatch(r"developer:\w+", who):
            raise ValueError("`as` is service, ci or developer:<user>")
        return who

    @field_validator("expect", mode="before")
    @classmethod
    def ok_means_success(cls, value: object) -> object:
        return "success" if value == "ok" else value


@dataclass(frozen=True)
class Identity:
    user: str
    target: str
    image_tag_env: str  # whose deployed tag provides the project
    dev_image: bool = False
    extra: tuple[str, ...] = ()


def identity(who: str, env: str) -> Identity:
    if who == "ci":
        return Identity("svc_ci_dbt", "ci", "dev", extra=("--vars", CI_VARS))
    if who.startswith("developer:"):
        return Identity(who.split(":", 1)[1], "default", env, dev_image=True)
    return Identity(f"svc_{env}_dbt", "run", env)


def dbt_args(command: str, who: Identity) -> list[str]:
    """`dbt <command> --target <t> [--vars ...]`; a target already named in the command wins."""
    args = shlex.split(command)
    if "--target" not in args and "-t" not in args:
        args += ["--target", who.target]
    if "--vars" not in args:
        args += list(who.extra)
    return ["dbt", *args]


def run_dbt(ctx: Ctx, env: str, who: Identity, command: str, timeout: float = 1800) -> clients.Proc:
    tag = clients.deployed_tag(ctx, who.image_tag_env)
    image = f"dbt-vdp:{tag}"
    project = clients.project_tar(ctx, image) if who.dev_image else None
    return clients.run_container(
        ctx,
        DEV_IMAGE if who.dev_image else image,
        dbt_args(command, who),
        env={"DBT_DEPLOY_TARGET": env, "SINGLESTORE_HOST": ctx.settings.s2_host, "SINGLESTORE_USER": who.user},
        secret_env={"SINGLESTORE_PASS": clients.password_for(ctx.settings, who.user)},
        project=project,
        timeout=timeout,
    )


def finished_line(output: str) -> str | None:
    """dbt's closing `Finished 'build' ...` line; without it dbt did not get as far as running anything."""
    found = [line.strip() for line in output.splitlines() if FINISHED.search(line)]
    return found[-1] if found else None


def plain_command(command: str) -> str:
    return PLAIN_COMMANDS.get(shlex.split(command)[0] if command.strip() else "", "step")


def touched_for(who: str, env: str) -> set[str]:
    if who == "ci":
        return {"ci"}
    if who.startswith("developer:"):
        return {f"developer:{env}:{who.split(':', 1)[1]}"}
    return {f"models:{env}"}


def tail(proc: clients.Proc, lines: int = 40) -> str:
    return "\n".join(proc.text.splitlines()[-lines:])


@action("dbt.run", DbtRunParams, lambda p: f"Running the {plain_command(p.command)}")
def dbt_run(ctx: Ctx, p: DbtRunParams) -> Result:
    proc = run_dbt(ctx, ctx.env, identity(p.as_, ctx.env), p.command, p.timeout)
    line = finished_line(proc.text)
    outcome = "success" if proc.returncode == 0 else "failed"
    what = plain_command(p.command)
    details = f"dbt {p.command} (exit code {proc.returncode})\n{tail(proc)}"
    touched = touched_for(p.as_, ctx.env)
    if line is None and proc.returncode != 0:
        raise StepFailed(f"The {what} could not even start.", details, touched)
    if outcome != p.expect:
        if p.expect == "failed":
            raise StepFailed(f"The {what} was expected to fail but finished without problems.", details, touched)
        raise StepFailed(f"The {what} finished with problems.", details, touched)
    summary = (
        f"The {what} finished without problems."
        if outcome == "success"
        else f"The {what} stopped with problems, as expected."
    )
    return Result(summary, details=details, touched=touched, data={"outcome": outcome, "finished": line or ""})


def clean_run(ctx: Ctx, env: str, who: Identity, command: str, done: str, failed: str) -> Result:
    proc = run_dbt(ctx, env, who, command)
    if proc.returncode != 0:
        raise StepFailed(failed, tail(proc))
    return Result(done, details=tail(proc))


@cleaner("models")
def clean_models(ctx: Ctx, resource: str) -> Result:
    _, env = resource.split(":")
    where = clients.ENV_WORDS[env]
    return clean_run(
        ctx,
        env,
        identity("service", env),
        "build --full-refresh",
        f"Rebuilt the reports in {where} from scratch.",
        f"The reports in {where} could not be rebuilt.",
    )


@cleaner("developer")
def clean_developer(ctx: Ctx, resource: str) -> Result:
    _, env, user = resource.split(":")
    return clean_run(
        ctx,
        env,
        identity(f"developer:{user}", env),
        "run-operation cleanup_user_models",
        f"Removed the test tables of {names.account_words(user) or user}.",
        f"The test tables of {names.account_words(user) or user} could not be removed.",
    )


@cleaner("ci")
def clean_ci(ctx: Ctx, resource: str) -> Result:
    return clean_run(
        ctx,
        "dev",
        identity("ci", "dev"),
        "run-operation drop_nodes",
        "Emptied the sandbox.",
        "The sandbox could not be emptied.",
    )
