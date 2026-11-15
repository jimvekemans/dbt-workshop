"""`docs.read_description`: what the documentation site of an environment says about one report.

Every environment has its own documentation site (the container `workshop-dbt-docs`, one port per environment). The
site is static: `dbt docs generate` writes its data as parquet files, which are fetched over HTTP here exactly like
the pages do. The description is read from `info_schema/v1/dbt.models.parquet`; the version is the one the environment
runs (the Airflow setting the deploy job writes right after the site). To show that the site belongs to that version,
the same sentence is looked up in the package `dbt-vdp:<version>` (its yml file), and the action says so when they differ.
"""

from __future__ import annotations

import io
import re
from typing import Any, Literal

import requests
import yaml
from pydantic import field_validator

from . import clients
from ..names import local_time_words
from .base import Ctx, Params, Result, StepFailed, action

DOCS_PORTS = {"dev": 8081, "val": 8082, "prd": 8086}
MODELS_FILE = "info_schema/v1/dbt.models.parquet"
GENERATED = re.compile(r'"generated_at":"([^"]+)"')


class ReadDescriptionParams(Params):
    model: str = "mart_olist_management_kpi"
    file: str = "models/im_dimensional/olist_management.yml"   # the yml of that report inside the package
    envs: list[Literal["dev", "val", "prd"]] = ["dev", "val"]
    expect: Literal["same", "different"] | None = None          # the first two sites must say the same / not the same

    @field_validator("file")
    @classmethod
    def inside_the_project(cls, path: str) -> str:
        if not re.fullmatch(r"models/[\w/]+\.ya?ml", path):
            raise ValueError("`file` is a yml file below models/")
        return path


def description_in(parquet: bytes, model: str) -> str | None:
    """The description of `model` in the documentation's table of tables, None when it is not there."""
    import pyarrow.parquet as pq   # a big library: loaded only when a documentation site is read

    table = pq.read_table(io.BytesIO(parquet), columns=["name", "description"])
    names, texts = table.column("name").to_pylist(), table.column("description").to_pylist()
    return next((text for name, text in zip(names, texts) if name == model), None)


def description_in_yml(text: str, model: str) -> str | None:
    for entry in (yaml.safe_load(text) or {}).get("models", []):
        if entry.get("name") == model:
            return entry.get("description")
    return None


def site_url(settings: Any, env: str, path: str) -> str:
    return f"http://{settings.docs_host}:{DOCS_PORTS[env]}/{path}"


def fetch(settings: Any, env: str, path: str) -> requests.Response:
    try:
        r = requests.get(site_url(settings, env, path), timeout=10)
    except requests.RequestException as exc:
        raise StepFailed(f"The documentation page of {clients.ENV_WORDS[env]} is not reachable.", str(exc)[:300]) from exc
    if r.status_code != 200:
        raise StepFailed(f"The documentation page of {clients.ENV_WORDS[env]} has no data yet.", f"{path}: {r.status_code}")
    return r


def package_description(ctx: Ctx, tag: str, p: ReadDescriptionParams) -> str | None:
    proc = clients.docker_run(ctx, "--entrypoint", "cat", f"dbt-vdp:{tag}", p.file, check=False, timeout=60)
    return description_in_yml(proc.stdout, p.model) if proc.returncode == 0 else None


def read_site(ctx: Ctx, env: str, p: ReadDescriptionParams) -> dict[str, Any]:
    tag = clients.deployed_tag(ctx, env)
    text = description_in(fetch(ctx.settings, env, MODELS_FILE).content, p.model)
    if text is None:
        raise StepFailed(f"The documentation of {clients.ENV_WORDS[env]} does not describe this report.", p.model)
    made = GENERATED.search(fetch(ctx.settings, env, "index.html").text)
    return {"env": env, "label": clients.ENV_WORDS[env].capitalize(), "version": tag, "description": text,
            "written_at": made[1] if made else None, "matches_package": package_description(ctx, tag, p) == text}


def lines_of(site: dict[str, Any]) -> list[str]:
    """Two plain lines per environment: which version it runs and when its documentation page was written, then the
    sentence the page shows. Whether that is the text of the package is a detail, unless it is not."""
    when = local_time_words(site["written_at"])
    head = f"{site['label']}: version {site['version']}" + (f", documentation page written {when}" if when else "")
    return [head, site["description"]] + ([] if site["matches_package"] else ["This is not the text of that version."])


@action("docs.read_description", ReadDescriptionParams,
        lambda p: "Reading the description of the report on the documentation pages of " + " and ".join(clients.ENV_WORDS[e] for e in p.envs))
def read_description(ctx: Ctx, p: ReadDescriptionParams) -> Result:
    sites = [read_site(ctx, env, p) for env in p.envs]
    same = len({s["description"] for s in sites[:2]}) == 1
    if p.expect and (p.expect == "same") != same:
        raise StepFailed(
            "The documentation pages say the same thing." if same else "The documentation pages do not say the same thing.",
            "\n".join(line for s in sites for line in lines_of(s)))
    names = " and ".join(clients.ENV_WORDS[e] for e in p.envs)
    return Result(
        f"Read the description of the report on the documentation pages of {names}.",
        details="\n".join(f"{s['env']}: {s['version']} written {s['written_at']}, same text as in the package: "
                          f"{s['matches_package']}" for s in sites),
        data={"reasons": [line for s in sites for line in lines_of(s)], "sites": sites, "same": same},
    )
