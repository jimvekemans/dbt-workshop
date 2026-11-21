"""`image.versions`: list the tools of the developer toolbox, and compare them with a second fresh toolbox or with the
package that runs in production.

The probes are a fixed, named set: a scenario picks names, never commands. All probes of one image run in ONE
throwaway container (`docker create` + `start` + `rm`, so Cancel and failures leave nothing behind), as one fixed
shell script built from the table below. Nothing is changed anywhere, so there is no cleaner.
"""

from __future__ import annotations

import re
from typing import Any, Literal

from pydantic import field_validator

from . import clients
from .base import Ctx, Params, Result, StepFailed, action
from .dbt import DEV_IMAGE

CONNECTOR_FILE = "/usr/local/lib/dbt/libadbc_driver_singlestore.so"
# name -> (plain name, the command, what to read from its output). The command is fixed text, never from a scenario.
PROBES: dict[str, tuple[str, str, str]] = {
    "report_builder": ("Rapportbouwer", "dbt --version", r"(\d+\.\d+\.\d+\S*)"),
    "connector": ("Databaseconnector", f"sha256sum {CONNECTOR_FILE}", r"^([0-9a-f]{12})"),
    "layout_robot": ("Layoutrobot", "sqlfmt --version", r"(\d+\.\d+\.\d+\S*)"),
    "rule_checker": ("Regelchecker", "dbt-bouncer --version", r"(\d+\.\d+\.\d+\S*)"),
    "helper_tool": ("Hulptool", "vdp --version", r"(\d+\.\d+\.\d+\S*)"),
    "pre_save_checks": ("Checks voor het opslaan", "pre-commit --version", r"(\d+\.\d+\.\d+\S*)"),
}
# The English names of the probes, for the technical details only (they stay as they were).
DETAIL_NAMES = {"report_builder": "Report builder", "connector": "Database connector", "layout_robot": "Layout robot",
                "rule_checker": "Rule checker", "helper_tool": "Helper tool", "pre_save_checks": "Pre-save checks"}
# The technical names, for the details only.
TECHNICAL = {"report_builder": "dbt", "connector": "ADBC driver", "layout_robot": "sqlfmt", "rule_checker": "dbt-bouncer",
             "helper_tool": "vdp", "pre_save_checks": "pre-commit"}
ProbeName = Literal["report_builder", "connector", "layout_robot", "rule_checker", "helper_tool", "pre_save_checks"]
MARK = "@@probe "
SCRIPT = "; ".join(f"echo '{MARK}{name}'; {cmd} 2>/dev/null | head -n 1 || true" for name, (_, cmd, _) in PROBES.items())


class ImageVersionsParams(Params):
    image: Literal["toolbox"] = "toolbox"
    compare_with: Literal["production", "toolbox"] | None = None
    probes: list[ProbeName] = list(PROBES)
    expect_same: list[ProbeName] = []   # these probes must give the same version on both sides, or the step fails

    @field_validator("probes")
    @classmethod
    def at_least_one(cls, probes: list[str]) -> list[str]:
        if not probes:
            raise ValueError("name at least one probe")
        return probes


NO_NUMBER = "unknown"   # the tool answered, but with no version number


def version_of(name: str, output: str | None) -> str | None:
    """The version a probe printed, `NO_NUMBER` when the tool answered without one, None when it is not in the image."""
    if not output or not output.strip():
        return None
    found = re.search(PROBES[name][2], output.strip())
    return found[1] if found else NO_NUMBER


def parse(output: str) -> dict[str, str | None]:
    """`{probe: version}` from the marked output of SCRIPT."""
    sections: dict[str, str] = {}
    current = None
    for line in output.splitlines():
        if line.startswith(MARK):
            current = line[len(MARK):].strip()
            sections[current] = ""
        elif current:
            sections[current] += line + "\n"
    return {name: version_of(name, sections.get(name)) for name in PROBES}


def list_tools(ctx: Ctx, image: str) -> dict[str, str | None]:
    proc = clients.run_container(ctx, image, ["sh", "-c", SCRIPT], timeout=180)
    if proc.returncode != 0 and MARK not in proc.stdout:
        raise StepFailed("Een verse container van het package kon niet geopend worden.", proc.text[-400:])
    return parse(proc.stdout)


INSTALLED = "geïnstalleerd"


def shown(name: str, version: str | None, *, side: str = "single", same: bool = True) -> str:
    """What a visitor reads for one tool: "versie 1.2.3"; "geïnstalleerd" for a tool without a real number (none printed,
    or 0.0.0 of a build from the team's own copy). The connector is a file: it reads "geïnstalleerd" in a single listing,
    "dezelfde build" when both sides have it and it matches, else "deze build" (left) and "een andere build" (right).
    "niet inbegrepen" when the tool is not there. A hash never shows."""
    if version is None:
        return "niet inbegrepen"
    if name == "connector":
        return INSTALLED if side == "single" else "dezelfde build" if same else (
            "deze build" if side == "left" else "een andere build")
    if version == NO_NUMBER or re.match(r"0\.0\.0(?![.\d])", version):
        return INSTALLED
    return f"versie {version}"


def technical_line(name: str, version: str | None) -> str:
    return f"{DETAIL_NAMES[name]} ({TECHNICAL[name]}): " + ("not included" if version is None else
                                                          f"build {version[:7]}" if name == "connector" else version)


@action("image.versions", ImageVersionsParams,
        lambda p: {"production": "De toolbox vergelijken met productie", "toolbox": "Twee verse toolboxen vergelijken"}.get(
            p.compare_with, "Een verse toolbox openen"))
def versions(ctx: Ctx, p: ImageVersionsParams) -> Result:
    left = list_tools(ctx, DEV_IMAGE)
    if not p.compare_with:
        rows = [{"probe": n, "label": PROBES[n][0], "left": left[n]} for n in p.probes]
        reasons = [{"label": r["label"], "left": shown(r["probe"], r["left"], side="single")} for r in rows]
        return Result("Een verse toolbox geopend en de tools opgelijst.",
                      details="\n".join([f"image {DEV_IMAGE}"] + [technical_line(r["probe"], r["left"]) for r in rows]),
                      data={"reasons": reasons, "left_title": "Toolbox", "tools": rows})
    tag = None
    if p.compare_with == "production":
        tag = clients.deployed_tag(ctx, "prd")
        other_image, titles = f"dbt-vdp:{tag}", ("Toolbox", "Productiepackage")
    else:
        other_image, titles = DEV_IMAGE, ("Eerste toolbox", "Tweede toolbox")
    right = list_tools(ctx, other_image)
    rows = []
    for n in p.probes:
        mine, theirs = left[n], right[n]
        state = "only_toolbox" if theirs is None else "same" if mine == theirs else "different"
        rows.append({"probe": n, "label": PROBES[n][0], "left": mine, "right": theirs, "state": state})
    reasons = [{"label": r["label"], "left": shown(r["probe"], r["left"], side="left", same=r["state"] == "same"),
                "right": None if r["right"] is None else shown(r["probe"], r["right"], side="right", same=r["state"] == "same"),
                "same": r["state"] == "same"} for r in rows]
    if odd := [r["label"] for r in rows if r["probe"] in p.expect_same and r["state"] != "same"]:
        raise StepFailed(f"Tussen beide is er een verschil bij: {', '.join(odd).lower()}.",
                         "\n".join(f"{DETAIL_NAMES[r['probe']]}: {r['state']}" for r in rows))
    first = "Twee verse toolboxen geopend en hun tools vergeleken." if p.compare_with == "toolbox" else \
        "Een verse toolbox vergeleken met het package dat in productie runt."
    detail = [f"{DEV_IMAGE} against {other_image}"] + [
        f"{technical_line(r['probe'], r['left'])} | {technical_line(r['probe'], r['right']).split(': ', 1)[1]} | {r['state']}"
        for r in rows]
    data: dict[str, Any] = {"reasons": reasons, "left_title": titles[0], "right_title": titles[1], "tools": rows}
    if tag:
        data["production_version"] = tag
    return Result(first, details="\n".join(detail), data=data)
