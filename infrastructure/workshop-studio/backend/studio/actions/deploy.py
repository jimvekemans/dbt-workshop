"""`deploy.set_version`: make an environment run a given numbered version, exactly like the CI deploy job does.

The CI job `.deploy` (dbt-singlestore-workshop/.gitlab-ci.yml) copies the schedules and helper code out of the image
`dbt-vdp:<tag>` into the environment's state volume, regenerates the documentation site and sets the Airflow setting
`dbt_image_tag`. This action repeats those three steps without a pipeline; the cleaner `version:<env>` does the same
for the baseline version.
"""

from __future__ import annotations

import re

from pydantic import field_validator

from . import clients
from .base import Cancelled, Ctx, Params, Result, StepFailed, action, cleaner

SEMVER = re.compile(r"v(\d+)\.(\d+)\.(\d+)")
RELEASE = re.compile(r"(\d{4})-Q([1-4])\.(\d+)")
COPY_CODE = (
    "find /state/dags -mindepth 1 -delete && mkdir -p /state/dags/utils && "
    "cp -r airflow/dags/. /state/dags/ && cp -r airflow/utils/. /state/dags/utils/ && "
    "find /state/dags -name __pycache__ -prune -exec rm -rf {} + ; chmod -R a+rwX /state/dags"
)
BUILD_DOCS = (
    "mkdir -p /state/docs && find /state/docs -mindepth 1 -delete && "
    "dbt docs generate --target run --output-dir /state/docs && chmod -R a+rwX /state/docs"
)
RELATIVE = {"baseline": "the original", "previous": "the previous", "latest": "the newest"}


class SetVersionParams(Params):
    tag: str  # baseline, previous, latest or a literal tag such as v1.1.0 or 2026-Q1.0

    @field_validator("tag")
    @classmethod
    def plausible(cls, tag: str) -> str:
        if tag not in RELATIVE and not (SEMVER.fullmatch(tag) or RELEASE.fullmatch(tag)):
            raise ValueError("a version is baseline, previous, latest, vX.Y.Z or YYYY-Qn.N")
        return tag


def version_key(tag: str) -> tuple[int, tuple[int, ...]] | None:
    """Sort key: development versions (v1.2.3) come before release versions (2026-Q1.0); None for other tags."""
    if found := SEMVER.fullmatch(tag):
        return 0, tuple(int(n) for n in found.groups())
    if found := RELEASE.fullmatch(tag):
        return 1, tuple(int(n) for n in found.groups())
    return None


def family(env: str, tags: list[str], baseline: str) -> list[str]:
    """The versions an environment can run, oldest first: development runs vX.Y.Z, validation and production run
    release tags (and may fall back to the baseline)."""
    own = [t for t in tags if version_key(t) and (version_key(t)[0] == 0) == (env == "dev")]
    if env != "dev" and baseline in tags:
        own.append(baseline)
    return sorted(set(own), key=lambda t: version_key(t) or (9, ()))


def pick_version(spec: str, env: str, current: str | None, tags: list[str], baseline: str) -> str:
    """Resolve baseline / previous / latest / a literal tag against the images that were built."""
    if spec == "baseline":
        return baseline
    if spec not in RELATIVE:
        if spec not in tags:
            raise StepFailed(f"Version {spec} has not been built yet.", f"images available: {sorted(tags)}")
        return spec
    options = family(env, tags, baseline)
    if not options:
        raise StepFailed(
            f"No version has been built for {clients.ENV_WORDS[env]} yet.", f"images available: {sorted(tags)}"
        )
    if spec == "latest":
        return options[-1]
    now = version_key(current) if current else None
    older = [t for t in options if now and version_key(t) < now]
    return older[-1] if older else baseline


def built_tags(ctx: Ctx) -> list[str]:
    proc = clients.docker(ctx, "image", "ls", "dbt-vdp", "--format", "{{.Tag}}")
    return [t for t in proc.stdout.split() if t != "<none>"]


def current_version(ctx: Ctx, env: str) -> str | None:
    proc = clients.airflow_cli(ctx, env, "variables", "get", "dbt_image_tag", check=False)
    return proc.stdout.strip() or None if proc.returncode == 0 else None


def deploy(ctx: Ctx, env: str, tag: str) -> None:
    image = f"dbt-vdp:{tag}"
    ctx.progress(f"Copying the schedules of version {tag} into {clients.ENV_WORDS[env]}")
    clients.docker_run(ctx, "-u", "root", "-v", f"workshop_{env}:/state", image, "sh", "-c", COPY_CODE)
    ctx.progress(f"Writing the documentation of version {tag}")
    password = clients.password_for(ctx.settings, f"svc_{env}_dbt")
    clients.docker_run(
        ctx,
        "-u",
        "root",
        "--network",
        ctx.settings.docker_network,
        "-v",
        f"workshop_{env}:/state",
        "-e",
        f"DBT_DEPLOY_TARGET={env}",
        "-e",
        f"SINGLESTORE_HOST={ctx.settings.s2_host}",
        "-e",
        f"SINGLESTORE_USER=svc_{env}_dbt",
        "-e",
        "SINGLESTORE_PASS",
        image,
        "sh",
        "-c",
        BUILD_DOCS,
        timeout=900,
        env={"SINGLESTORE_PASS": password},
    )
    ctx.progress(f"Telling {clients.ENV_WORDS[env]} to run version {tag}")
    clients.airflow_cli(ctx, env, "variables", "set", "dbt_image_tag", tag)


@action("deploy.set_version", SetVersionParams, lambda p: f"Putting {RELATIVE.get(p.tag, p.tag)} version live")
def set_version(ctx: Ctx, p: SetVersionParams) -> Result:
    tag = pick_version(p.tag, ctx.env, current_version(ctx, ctx.env), built_tags(ctx), ctx.settings.baseline_tag)
    try:
        deploy(ctx, ctx.env, tag)
    except (StepFailed, Cancelled) as e:   # files or the setting may already have changed
        e.touched = set(getattr(e, "touched", ())) | {f"version:{ctx.env}"}
        raise
    return Result(
        f"{clients.ENV_WORDS[ctx.env].capitalize()} now runs version {tag}.",
        details=f"dbt_image_tag = {tag}; schedules and docs copied from image dbt-vdp:{tag}",
        touched={f"version:{ctx.env}"},
        data={"tag": tag},
    )


@cleaner("version")
def restore_version(ctx: Ctx, resource: str) -> Result:
    _, env = resource.split(":")
    baseline = ctx.settings.baseline_tag
    if current_version(ctx, env) == baseline:
        return Result(f"{clients.ENV_WORDS[env].capitalize()} already runs the original version.")
    deploy(ctx, env, baseline)
    return Result(f"{clients.ENV_WORDS[env].capitalize()} is back on the original version {baseline}.")
