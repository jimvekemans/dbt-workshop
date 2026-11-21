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
from ..names import format_number, local_time_words
from .base import Ctx, Params, Result, StepFailed, action

DOCS_PORTS = {"dev": 8081, "val": 8082, "prd": 8086}
MODELS_FILE = "info_schema/v1/dbt.models.parquet"
BLOCKS_FILE = "info_schema/v1/dbt.docs_blocks.parquet"
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
        raise StepFailed(f"De documentatiepagina van {clients.ENV_WORDS[env]} is niet bereikbaar.", str(exc)[:300]) from exc
    if r.status_code != 200:
        raise StepFailed(f"De documentatiepagina van {clients.ENV_WORDS[env]} heeft nog geen data.", f"{path}: {r.status_code}")
    return r


def package_description(ctx: Ctx, tag: str, p: ReadDescriptionParams) -> str | None:
    proc = clients.docker_run(ctx, "--entrypoint", "cat", f"dbt-vdp:{tag}", p.file, check=False, timeout=60)
    return description_in_yml(proc.stdout, p.model) if proc.returncode == 0 else None


def read_site(ctx: Ctx, env: str, p: ReadDescriptionParams) -> dict[str, Any]:
    tag = clients.deployed_tag(ctx, env)
    text = description_in(fetch(ctx.settings, env, MODELS_FILE).content, p.model)
    if text is None:
        raise StepFailed(f"De documentatie van {clients.ENV_WORDS[env]} beschrijft dit rapport niet.", p.model)
    made = GENERATED.search(fetch(ctx.settings, env, "index.html").text)
    return {"env": env, "label": clients.ENV_WORDS[env].capitalize(), "version": tag, "description": text,
            "written_at": made[1] if made else None, "matches_package": package_description(ctx, tag, p) == text}


def lines_of(site: dict[str, Any]) -> list[str]:
    """Two plain lines per environment: which version it runs and when its documentation page was written, then the
    sentence the page shows. Whether that is the text of the package is a detail, unless it is not."""
    when = local_time_words(site["written_at"])
    head = f"{site['label']}: versie {site['version']}" + (f", documentatiepagina geschreven op {when}" if when else "")
    return [head, site["description"]] + ([] if site["matches_package"] else ["Dit is niet de tekst van die versie."])


@action("docs.read_description", ReadDescriptionParams,
        lambda p: "De beschrijving van het rapport lezen op de documentatiepagina's van " + " en ".join(clients.ENV_WORDS[e] for e in p.envs))
def read_description(ctx: Ctx, p: ReadDescriptionParams) -> Result:
    sites = [read_site(ctx, env, p) for env in p.envs]
    same = len({s["description"] for s in sites[:2]}) == 1
    if p.expect and (p.expect == "same") != same:
        raise StepFailed(
            "De documentatiepagina's zeggen hetzelfde." if same else "De documentatiepagina's zeggen niet hetzelfde.",
            "\n".join(line for s in sites for line in lines_of(s)))
    names = " en ".join(clients.ENV_WORDS[e] for e in p.envs)
    return Result(
        f"De beschrijving van het rapport gelezen op de documentatiepagina's van {names}.",
        details="\n".join(f"{s['env']}: {s['version']} written {s['written_at']}, same text as in the package: "
                          f"{s['matches_package']}" for s in sites),
        data={"reasons": [line for s in sites for line in lines_of(s)], "sites": sites, "same": same},
    )


# --- the front page ----------------------------------------------------------------------------------------------
# The deploy writes the front page of the site (scripts/docs_overview.sh: a `{% docs __overview__ %}` block in the
# project, so its package is the project's, not dbt's). Without it the site shows dbt's standard welcome text.

OVERVIEW = "__overview__"
VERSION_LINE = re.compile(r"\*\*Version:\*\*\s*`([^`]+)`")
GENERATED_LINE = re.compile(r"\*\*Generated:\*\*\s*(\d{4}-\d\d-\d\d) (\d\d:\d\d)(?::(\d\d))? UTC")   # seconds only from newer versions of the script
SIZE_LINE = re.compile(r"\*\*Tables:\*\*\s*(\w+), \*\*checks:\*\*\s*(\w+), \*\*feeds:\*\*\s*(\w+)")


class FrontPageParams(Params):
    envs: list[Literal["dev", "val", "prd"]] = ["dev"]
    expect: Literal["running"] | None = None   # the step fails when the page names another version than the one that runs


def front_page_in(parquet: bytes) -> str | None:
    """The text of the project's own front page block; None when the site has only dbt's standard one."""
    import pyarrow.parquet as pq

    table = pq.read_table(io.BytesIO(parquet), columns=["name", "package_name", "content"])
    rows = [r for r in table.to_pylist() if r["name"] == OVERVIEW]
    own = [r for r in rows if r["package_name"] != "dbt"]
    return own[0]["content"] if own else None


def count_of(text: str) -> int | None:
    return int(text) if text.isdigit() else None


def parse_front_page(content: str | None) -> dict[str, Any]:
    """`{written, version, written_at, tables, checks, feeds, teams}` from the page text (`written` False for the
    standard page; a number the page could not count is None)."""
    if content is None:
        return {"written": False, "version": None, "written_at": None, "with_seconds": False, "tables": None, "checks": None,
                "feeds": None, "teams": None}
    version, made, size = VERSION_LINE.search(content), GENERATED_LINE.search(content), SIZE_LINE.search(content)
    teams = content.split("## Teams", 1)[1] if "## Teams" in content else ""
    return {
        "written": True,
        "version": version[1] if version else None,
        "written_at": f"{made[1]}T{made[2]}:{made[3] or '00'}Z" if made else None,
        "with_seconds": bool(made and made[3]),
        "tables": count_of(size[1]) if size else None,
        "checks": count_of(size[2]) if size else None,
        "feeds": count_of(size[3]) if size else None,
        "teams": len(re.findall(r"^- `", teams, re.MULTILINE)),
    }


PLURALS = {"tabel": "tabellen"}   # the other nouns of the page take an "s"


def number_words(n: int | None, noun: str) -> str:
    many = PLURALS.get(noun, noun + "s")
    return f"{format_number(n)} {noun if n == 1 else many}" if n is not None else f"geen telling van {many}"


def front_page_lines(label: str, page: dict[str, Any]) -> list[str]:
    if not page["written"]:
        return [f"{label}: de startpagina is de standaardpagina. Ze vermeldt geen versie, geen tijdstip en geen grootte."]
    when = local_time_words(page["written_at"])
    if when and page.get("with_seconds"):
        when += ":" + page["written_at"][-3:-1]   # the page has the second too: "19:14:35"
    head = f"{label}: de startpagina vermeldt versie {page['version'] or 'onbekend'}" + (f", geschreven op {when}." if when else ".")
    sizes = [number_words(page[k], noun) for k, noun in (("tables", "tabel"), ("checks", "check"), ("feeds", "feed"))]
    size = ", ".join(sizes[:-1]) + " en " + sizes[-1]
    return [head, f"Ze telt {size} en noemt {number_words(page['teams'], 'team')}."]


@action("docs.read_front_page", FrontPageParams,
        lambda p: "De startpagina van de documentatie van " + " en ".join(clients.ENV_WORDS[e] for e in p.envs) + " lezen")
def read_front_page(ctx: Ctx, p: FrontPageParams) -> Result:
    pages = []
    for env in p.envs:
        page = parse_front_page(front_page_in(fetch(ctx.settings, env, BLOCKS_FILE).content))
        pages.append({"env": env, "label": clients.ENV_WORDS[env].capitalize(), "running": clients.deployed_tag(ctx, env), **page})
    if p.expect == "running":
        for page in pages:
            if page["version"] != page["running"]:
                raise StepFailed(
                    f"De startpagina van {clients.ENV_WORDS[page['env']]} vermeldt niet de versie die daar runt.",
                    f"front page: {page['version']}, running: {page['running']}")
    names_ = " en ".join(clients.ENV_WORDS[e] for e in p.envs)
    return Result(
        f"De startpagina van de documentatie van {names_} gelezen.",
        details="\n".join(f"{pg['env']}: version {pg['version']} (runs {pg['running']}) written {pg['written_at']} "
                          f"tables {pg['tables']} checks {pg['checks']} feeds {pg['feeds']} teams {pg['teams']}" for pg in pages),
        data={"reasons": [line for pg in pages for line in front_page_lines(pg["label"], pg)], "pages": pages},
    )
