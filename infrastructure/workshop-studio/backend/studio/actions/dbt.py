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
# Each phrase carries its article ("de ...", "het ...") and fits "{What} is zonder problemen afgelopen" and "Bezig met {what}".
PLAIN_COMMANDS = {
    "build": "de build van de rapporten",
    "run": "de build van de rapporten",
    "test": "de run van de datachecks",
    "seed": "het laden van referentiedata",
    "snapshot": "de snapshot van de historiek",
    "run-operation": "de opkuisstap",
    "source": "de controle van de feeds",
    "compile": "de controle van de code",
    "parse": "de controle van de code",
    "retry": "de retry van het mislukte deel",
}


class DbtRunParams(Params):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    as_: str = Field("service", alias="as")
    command: str
    expect: Literal["success", "failed"] = "success"
    timeout: int = 1800
    explain: bool = False   # also say, in one plain line for a `result` card, how many tables were built and checks run

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
    return PLAIN_COMMANDS.get(shlex.split(command)[0] if command.strip() else "", "de stap")


def upper_first(text: str) -> str:
    return text[:1].upper() + text[1:]


def touched_for(who: str, env: str) -> set[str]:
    if who == "ci":
        return {"ci"}
    if who.startswith("developer:"):
        return {f"developer:{env}:{who.split(':', 1)[1]}"}
    return {f"models:{env}"}


PROCESSED = re.compile(r"Processed:\s*(.+)")


def processed_words(output: str) -> str | None:
    """dbt's closing `Processed: 1 hook | 30 models | 36 tests` as "30 tabellen gebuild en 36 checks gerund."; None without it."""
    found = PROCESSED.findall(output)
    if not found:
        return None
    counts = {kind: int(n) for n, kind in re.findall(r"(\d+) (models?|tests?)", found[-1])}
    tables, checks = counts.get("model", 0) + counts.get("models", 0), counts.get("test", 0) + counts.get("tests", 0)
    plural = lambda n, one, many: f"{names.format_number(n)} {one if n == 1 else many}"  # noqa: E731
    return f"{plural(tables, 'tabel', 'tabellen')} gebuild en {plural(checks, 'check', 'checks')} gerund."


def tail(proc: clients.Proc, lines: int = 40) -> str:
    return "\n".join(proc.text.splitlines()[-lines:])


@action("dbt.run", DbtRunParams, lambda p: f"Bezig met {plain_command(p.command)}")
def dbt_run(ctx: Ctx, p: DbtRunParams) -> Result:
    proc = run_dbt(ctx, ctx.env, identity(p.as_, ctx.env), p.command, p.timeout)
    line = finished_line(proc.text)
    outcome = "success" if proc.returncode == 0 else "failed"
    what = upper_first(plain_command(p.command))
    details = f"dbt {p.command} (exit code {proc.returncode})\n{tail(proc)}"
    touched = touched_for(p.as_, ctx.env)
    if line is None and proc.returncode != 0:
        raise StepFailed(f"{what} kon niet eens starten.", details, touched)
    if outcome != p.expect:
        if p.expect == "failed":
            raise StepFailed(f"{what} had moeten mislukken, maar is zonder problemen afgelopen.", details, touched)
        raise StepFailed(f"{what} is met problemen geëindigd.", details, touched)
    summary = (
        f"{what} is zonder problemen afgelopen."
        if outcome == "success"
        else f"{what} is met problemen gestopt, zoals verwacht."
    )
    data = {"outcome": outcome, "finished": line or ""}
    if p.explain and (counted := processed_words(proc.text)):
        data["reasons"] = [counted]
    return Result(summary, details=details, touched=touched, data=data)


def clean_run(ctx: Ctx, env: str, who: Identity, command: str, done: str, failed: str) -> Result:
    proc = run_dbt(ctx, env, who, command)
    if proc.returncode != 0:
        raise StepFailed(failed, tail(proc))
    return Result(done, details=tail(proc))


@cleaner("models")
def clean_models(ctx: Ctx, resource: str) -> Result:
    """`models:<env>` rebuilds every table of the environment; `models:<env>:<table>` only that one (a demo that
    changed a single shared table, for example a rehearsal that was allowed to build there)."""
    _, env, *only = resource.split(":")
    where = clients.ENV_WORDS[env]
    if only:
        title = names.model_title(only[0])
        table = (lambda words: words[:1].upper() + words[1:])(names.with_article(title)) if title else "De tabel"
        return clean_run(
            ctx,
            env,
            identity("service", env),
            f"run --select {only[0]} --full-refresh",
            f"{table} in {where} is van nul herbouwd.",
            f"{table} in {where} kon niet herbouwd worden.",
        )
    return clean_run(
        ctx,
        env,
        identity("service", env),
        "build --full-refresh",
        f"De rapporten in {where} zijn van nul herbouwd.",
        f"De rapporten in {where} konden niet herbouwd worden.",
    )


@cleaner("developer")
def clean_developer(ctx: Ctx, resource: str) -> Result:
    _, env, user = resource.split(":")
    return clean_run(
        ctx,
        env,
        identity(f"developer:{user}", env),
        "run-operation cleanup_user_models",
        f"De testtabellen van {names.account_words(user) or user} zijn verwijderd.",
        f"De testtabellen van {names.account_words(user) or user} konden niet verwijderd worden.",
    )


def drop_rehearsal_copies(ctx: Ctx) -> Result:
    """`ci:copies`: every table a rehearsal left behind in the rehearsal database. Nothing else lives there."""
    conn = clients.s2_connect(ctx.settings, "root", timeout=30)
    try:
        with conn.cursor() as cur:
            cur.execute("select table_name, table_type from information_schema.tables where table_schema = 'vdp_ci'")
            found = list(cur.fetchall())
            for name, kind in found:
                cur.execute(f"drop {'view' if 'VIEW' in str(kind).upper() else 'table'} if exists `vdp_ci`.`{name}`")
    except Exception as exc:  # noqa: BLE001 - one plain sentence, the technical text in the details
        raise StepFailed("De kopieën van de testruns konden niet verwijderd worden.", str(exc)) from exc
    finally:
        conn.close()
    return Result("De kopieën die de testruns achterlieten, zijn verwijderd." if found else "Er waren geen kopieën van testruns meer.",
                  details=f"dropped {len(found)} relation(s) in vdp_ci")


@cleaner("ci")
def clean_ci(ctx: Ctx, resource: str) -> Result:
    if resource == "ci:copies":
        return drop_rehearsal_copies(ctx)
    return clean_run(
        ctx,
        "dev",
        identity("ci", "dev"),
        "run-operation drop_nodes",
        "De sandbox is leeggemaakt.",
        "De sandbox kon niet leeggemaakt worden.",
    )
