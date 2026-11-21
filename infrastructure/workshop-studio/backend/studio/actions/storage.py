"""`storage.*` and `package.open`: what the stores of a numbered version hold.

Every accepted change gets a number (a tag), is sealed into a package (the image `dbt-vdp:<tag>`) and, through the job
`publish_artifacts`, its description (the `target/` folder: the catalogue file under `target/collibra/` and the
documentation under `target/dbt-docs/`) is stored as `target.tar.gz` in GitLab's generic package registry: package
`dbt-artifacts`, version `<tag>`. Every deploy stores the same file again as the copy of the environment (package
`dbt-artifacts-env`, version `<env>`, overwritten in place) together with the note `deployed.json` (`version`, `commit`,
`environment`, `sha256`, `deployed_at`). A generic package keeps every uploaded file, so "overwritten" means a newer file
of the same name; the newest one is what a download returns.

Actions (all read only, so they touch nothing):

* `storage.versions`        the numbered versions: `show: numbers | codes` read the tags of the code server (the same
                            changes with and without their number), `stored` the versions kept in storage
* `storage.environment_copy` the environment's copy in storage: `show: copy` what is on it, `show: note` what its note says
                            and which stored version its fingerprint equals
* `storage.description`     the description of a report as stored with one version
* `storage.catalogue_names` table names in the stored catalogue file of one version, next to the names a developer's own
                            compile gives
* `package.open`            the sealed packages of one or two versions: the number printed on them, the tools in them and
                            the description in their code

The registry is also the place where demos leave things behind (a stored package per demo version, a newer copy of the
environment). `drop_orphans` and `restore_env_copy` put that back; the cleaners of `git` and `version` call them.
"""

from __future__ import annotations

import io
import json
import re
import tarfile
from typing import Any, Literal

from pydantic import Field, field_validator

from .. import names
from . import clients, docs, images
from .base import Ctx, Params, Result, StepFailed, action
from .deploy import RELATIVE, built_tags, family, version_key
from .dbt import identity, run_dbt

PACKAGE = "dbt-artifacts"
ENV_PACKAGE = "dbt-artifacts-env"
TARBALL = "target.tar.gz"
NOTE = "deployed.json"
CATALOGUE_FILE = "target/collibra/manifest.json"
DESCRIPTIONS_FILE = "target/dbt-docs/info_schema/v1/dbt.models.parquet"
FIRST_VERSION = "De eerste versie"
DEFAULT_TABLES = ["fct_olist_orders", "dim_olist_customers", "mart_olist_management_kpi"]


# --- the registry ----------------------------------------------------------------------------------------------


def registry_packages(api: clients.GitLabApi, name: str) -> list[dict[str, Any]]:
    """The generic packages called exactly `name` (GitLab's name filter also matches longer names)."""
    found = api.get("/packages", package_name=name, package_type="generic")
    return [p for p in found if p["name"] == name]


def files_of(api: clients.GitLabApi, package_id: int) -> list[dict[str, Any]]:
    """The files of a package, oldest first."""
    return sorted(api.get(f"/packages/{package_id}/package_files"), key=lambda f: (f["created_at"], f["id"]))


def newest(files: list[dict[str, Any]], name: str) -> dict[str, Any] | None:
    named = [f for f in files if f["file_name"] == name]
    return named[-1] if named else None


def download(api: clients.GitLabApi, package: str, version: str, name: str) -> bytes:
    """The newest file `name` of the package version (what a plain download of the registry returns)."""
    r = api.req("GET", api.p(f"/packages/generic/{package}/{clients.quote(version)}/{name}"), ok=(200, 404))
    if r.status_code != 200:
        raise StepFailed(f"Voor versie {version} staat niets in storage." if package == PACKAGE
                         else f"Van {clients.ENV_WORDS.get(version, version)} staat geen kopie in storage.",
                         f"{package}/{version}/{name}: {r.status_code}")
    return r.content


def size_words(size: int) -> str:
    return f"{size / 1_000_000:.1f}".replace(".", ",") + " MB"


def when_words(stamp: str | None) -> str:
    """"op 21 november, 18:06", or "op een onbekend tijdstip": it follows a verb like "opgeslagen"."""
    return "op " + (names.local_time_words(stamp) or "een onbekend tijdstip")


def stored_versions(api: clients.GitLabApi) -> list[dict[str, Any]]:
    """The versions in storage, oldest first: `{version, package_id, stored_at, size, sha256}`."""
    out = []
    for package in registry_packages(api, PACKAGE):
        if (file := newest(files_of(api, package["id"]), TARBALL)) is None or version_key(package["version"]) is None:
            continue
        out.append({"version": package["version"], "package_id": package["id"], "stored_at": file["created_at"],
                    "size": file["size"], "sha256": file["file_sha256"]})
    return sorted(out, key=lambda v: version_key(v["version"]) or (9, ()))


def change_title(api: clients.GitLabApi, commit: dict[str, Any]) -> str:
    """What the change behind a number is called. The first version has no change before it; an accepted change is a
    merge on `dev`, whose second parent is the squashed change with the title of the proposal."""
    parents = commit.get("parent_ids") or []
    if not parents:
        return FIRST_VERSION
    if len(parents) > 1 and (inner := api.maybe(f"/repository/commits/{parents[1]}")):
        return inner.get("title") or ""
    return commit.get("title") or ""


def tag_facts(api: clients.GitLabApi) -> dict[str, dict[str, Any]]:
    """`{tag: {code, title, when}}`: the change behind each number."""
    facts = {}
    for tag in api.get("/repository/tags"):
        commit = tag.get("commit") or {}
        facts[tag["name"]] = {
            "code": commit.get("short_id") or "",
            "title": change_title(api, commit),
            "when": commit.get("committed_date") or tag.get("created_at"),
        }
    return facts


def resolve(spec: str, env: str, have: list[str], baseline: str) -> str:
    """`baseline`, `previous` (the one before the newest stored), `latest` or a literal version, among `have`."""
    options = family(env, have, baseline)
    if spec == "baseline":
        wanted = baseline
    elif spec == "latest":
        wanted = options[-1] if options else ""
    elif spec == "previous":
        if len(options) < 2:
            raise StepFailed("Er is maar één versie opgeslagen, dus er is geen vorige.", f"stored: {sorted(have)}")
        wanted = options[-2]
    else:
        wanted = spec
    if wanted not in have:
        raise StepFailed(f"Versie {wanted or spec} staat niet in storage.", f"stored: {sorted(have)}")
    return wanted


def version_spec(spec: str) -> str:
    if spec not in RELATIVE and not version_key(spec):
        raise ValueError("a version is baseline, previous, latest, vX.Y.Z or YYYY-Qn.N")
    return spec


# --- storage.versions ------------------------------------------------------------------------------------------


class VersionsParams(Params):
    show: Literal["numbers", "codes", "stored"] = "numbers"


@action("storage.versions", VersionsParams, lambda p: "De genummerde versies lezen" if p.show != "stored" else "Lezen wat de storage bevat")
def versions(ctx: Ctx, p: VersionsParams) -> Result:
    api = clients.GitLabApi(ctx.settings)
    facts = tag_facts(api)
    if p.show == "stored":
        stored = stored_versions(api)
        if not stored:
            raise StepFailed("De storage bevat nog geen versie.")
        rows = [{**v, "title": facts.get(v["version"], {}).get("title", "")} for v in reversed(stored)]
        reasons = [f"{v['version']}: opgeslagen {when_words(v['stored_at'])}, {size_words(v['size'])}" for v in rows]
        return Result(
            f"De storage bevat {len(rows)} {'versie' if len(rows) == 1 else 'versies'}; de nieuwste is {rows[0]['version']}.",
            details="\n".join(f"{PACKAGE}/{v['version']}/{TARBALL}  {v['size']} bytes  sha256 {v['sha256']}" for v in rows),
            data={"reasons": reasons, "versions": rows})
    numbered = sorted((t for t in facts if version_key(t)), key=lambda t: version_key(t) or (9, ()), reverse=True)
    if not numbered:
        raise StepFailed("Er is nog geen versie genummerd.")
    if p.show == "numbers":
        reasons = [f"{t}: {facts[t]['title']}" for t in numbered]
        summary = (f"{len(numbered)} {'wijziging heeft' if len(numbered) == 1 else 'wijzigingen hebben'} een nummer; "
                   f"de nieuwste is {numbered[0]}.")
    else:   # the same changes as the code server knows them without a number: by their code, in no order
        by_code = sorted(numbered, key=lambda t: facts[t]["code"])
        reasons = [f"Code {facts[t]['code']}: {facts[t]['title']}" for t in by_code]
        summary = (f"Dezelfde {len(numbered)} {'wijziging' if len(numbered) == 1 else 'wijzigingen'}, "
                   "enkel gekend onder hun code.")
    return Result(summary, details="\n".join(f"{t} -> {facts[t]['code']} {facts[t]['title']}" for t in numbered),
                  data={"reasons": reasons, "versions": [{"version": t, **facts[t]} for t in numbered]})


# --- storage.environment_copy ----------------------------------------------------------------------------------


class EnvironmentCopyParams(Params):
    show: Literal["copy", "note"] = "note"
    expect_match: bool = False   # `note`: the step fails unless the fingerprint equals the stored file of exactly one version


def env_copy(api: clients.GitLabApi, env: str) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """The newest tarball of the environment's copy, and the files of its package."""
    package = next((p for p in registry_packages(api, ENV_PACKAGE) if p["version"] == env), None)
    files = files_of(api, package["id"]) if package else []
    tarball = newest(files, TARBALL)
    if tarball is None:
        raise StepFailed(f"Van {clients.ENV_WORDS[env]} staat nog geen kopie in storage.", f"{ENV_PACKAGE}/{env}")
    return tarball, files


@action("storage.environment_copy", EnvironmentCopyParams,
        lambda p: "De kopie van de omgeving in storage lezen" if p.show == "copy" else "De notitie bij de kopie lezen")
def environment_copy(ctx: Ctx, p: EnvironmentCopyParams) -> Result:
    api = clients.GitLabApi(ctx.settings)
    env, label = ctx.env, clients.ENV_WORDS[ctx.env]
    tarball, _ = env_copy(api, env)
    if p.show == "copy":
        return Result(
            f"De kopie van {label} in storage gelezen.",
            details=f"{ENV_PACKAGE}/{env}/{TARBALL}  {tarball['size']} bytes  sha256 {tarball['file_sha256']}",
            data={"reasons": [f"De kopie van {label} is opgeslagen {when_words(tarball['created_at'])} en is {size_words(tarball['size'])} groot.",
                              "Op de kopie zelf staat enkel de naam van de omgeving, geen versie."],
                  "stored_at": tarball["created_at"], "size": tarball["size"]})
    try:
        note = json.loads(download(api, ENV_PACKAGE, env, NOTE))
    except (ValueError, TypeError) as exc:
        raise StepFailed(f"De notitie bij de kopie van {label} kan niet gelezen worden.", str(exc)) from exc
    have = stored_versions(api)
    fingerprint = tarball["file_sha256"]
    matching = [v["version"] for v in have if v["sha256"] == fingerprint]
    reasons = [f"De notitie zegt: versie {note.get('version', 'onbekend')}, gedeployd {when_words(note.get('deployed_at'))}.",
               f"Fingerprint van de kopie: {fingerprint[:8]}"]
    reasons += [f"Fingerprint van het opgeslagen bestand van {v['version']}: {v['sha256'][:8]}" + (" (dezelfde)" if v["sha256"] == fingerprint else "")
                for v in reversed(have)]
    only = matching[0] if len(matching) == 1 else None
    if p.expect_match and (only is None or note.get("version") != only):
        raise StepFailed(
            "De kopie komt met geen enkele opgeslagen versie overeen." if not matching else "De kopie komt niet exact overeen met de versie uit de notitie.",
            f"note {note}\ncopy sha256 {fingerprint}\nstored: {[(v['version'], v['sha256']) for v in have]}")
    summary = (f"De kopie van {label} is gelijk aan het opgeslagen bestand van {only}, en aan "
               "geen enkele andere opgeslagen versie." if only and len(have) > 1 else
               f"De kopie van {label} is gelijk aan het opgeslagen bestand van {only}." if only else
               f"De kopie van {label} is gelijk aan geen enkele opgeslagen versie.")
    return Result(summary, details=f"note: {json.dumps(note)}\ncopy sha256 {fingerprint}\nmatching: {matching}",
                  data={"reasons": reasons, "note": note, "fingerprint": fingerprint, "matches": matching,
                        "stored": [v["version"] for v in have]})


# --- storage.description ---------------------------------------------------------------------------------------


class DescriptionParams(Params):
    version: str = "latest"
    model: str = "mart_olist_management_kpi"

    @field_validator("version")
    @classmethod
    def known_version(cls, spec: str) -> str:
        return version_spec(spec)


def read_tar(blob: bytes, member: str) -> bytes:
    with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as tar:
        try:
            handle = tar.extractfile(member)
        except KeyError:
            handle = None
        if handle is None:
            raise StepFailed("De opgeslagen beschrijving bevat niet wat verwacht werd.", f"missing in {TARBALL}: {member}")
        return handle.read()


@action("storage.description", DescriptionParams, lambda p: "Een opgeslagen beschrijving lezen")
def description(ctx: Ctx, p: DescriptionParams) -> Result:
    api = clients.GitLabApi(ctx.settings)
    have = stored_versions(api)
    tag = resolve(p.version, ctx.env, [v["version"] for v in have], ctx.settings.baseline_tag)
    stored = next(v for v in have if v["version"] == tag)
    text = docs.description_in(read_tar(download(api, PACKAGE, tag, TARBALL), DESCRIPTIONS_FILE), p.model)
    if text is None:
        raise StepFailed(f"De opgeslagen beschrijving van versie {tag} beschrijft dit rapport niet.", p.model)
    return Result(
        f"De beschrijving van het rapport uit de storage gelezen, zoals die bij versie {tag} is opgeslagen.",
        details=f"{PACKAGE}/{tag}/{TARBALL} -> {DESCRIPTIONS_FILE}, model {p.model}",
        data={"reasons": [f"Opgeslagen bij versie {tag} {when_words(stored['stored_at'])}:", text],
              "version": tag, "description": text, "stored_at": stored["stored_at"]})


# --- storage.catalogue_names -----------------------------------------------------------------------------------


class CatalogueNamesParams(Params):
    version: str = "latest"
    models: list[str] = Field(default_factory=lambda: list(DEFAULT_TABLES), min_length=1, max_length=5)
    compare_with: Literal["developer:pu_dev1", "developer:pu_dev2", "developer:pu_dev3"] | None = None

    @field_validator("version")
    @classmethod
    def known_version(cls, spec: str) -> str:
        return version_spec(spec)

    @field_validator("models")
    @classmethod
    def plain_model_names(cls, models: list[str]) -> list[str]:
        if bad := [m for m in models if not re.fullmatch(r"[a-z][a-z0-9_]*", m)]:
            raise ValueError(f"not a table name: {bad[0]}")
        return models


def catalogue_aliases(manifest: bytes, models: list[str]) -> dict[str, str]:
    """`{model: table name}` as the catalogue file lists them (the relation's own name)."""
    nodes = json.loads(manifest).get("nodes", {})
    found = {n["name"]: n.get("alias") or n["name"] for n in nodes.values()
             if n.get("resource_type") == "model" and n.get("name") in models}
    if missing := [m for m in models if m not in found]:
        raise StepFailed("Het catalogusbestand bevat niet elke gevraagde tabel.", f"missing: {', '.join(missing)}")
    return found


def compiled_aliases(ctx: Ctx, who: str, models: list[str]) -> dict[str, str]:
    """`{model: table name}` as a developer's own compile names them (the developer's prefix in front)."""
    command = f"ls -s {' '.join(models)} --output json --output-keys name alias"
    proc = run_dbt(ctx, ctx.env, identity(who, ctx.env), command, timeout=300)
    found = {}
    for line in proc.stdout.splitlines():
        if line.startswith("{"):
            try:
                node = json.loads(line)
            except ValueError:
                continue
            if node.get("name") in models and node.get("alias"):
                found[node["name"]] = node["alias"]
    if proc.returncode != 0 or len(found) != len(models):
        raise StepFailed("De eigen versie van een developer kon de tabellen niet benoemen.", proc.text[-400:])
    return found


@action("storage.catalogue_names", CatalogueNamesParams, lambda p: "Tabelnamen lezen in het opgeslagen catalogusbestand")
def catalogue_names(ctx: Ctx, p: CatalogueNamesParams) -> Result:
    api = clients.GitLabApi(ctx.settings)
    have = [v["version"] for v in stored_versions(api)]
    tag = resolve(p.version, ctx.env, have, ctx.settings.baseline_tag)
    stored = catalogue_aliases(read_tar(download(api, PACKAGE, tag, TARBALL), CATALOGUE_FILE), p.models)
    own = compiled_aliases(ctx, p.compare_with, p.models) if p.compare_with else {}
    rows = []
    for model in p.models:
        row = {"label": names.model_title(model) or model.replace("_", " ").capitalize(), "left": stored[model]}
        if p.compare_with:
            row.update(right=own[model], same=stored[model] == own[model])
        rows.append(row)
    summary = (f"{len(rows)} {'tabelnaam' if len(rows) == 1 else 'tabelnamen'} gelezen in het opgeslagen catalogusbestand van versie {tag}"
               + (" en in de eigen versie van een developer." if p.compare_with else "."))
    data = {"reasons": rows, "version": tag, "label_title": "Tabel", "left_title": "In het opgeslagen catalogusbestand",
            "right_title": "In de eigen versie van een developer", "stored": stored, "compiled": own}
    return Result(summary, details="\n".join(f"{m}: catalogue {stored[m]}" + (f", compile {own[m]}" if own else "") for m in p.models),
                  data=data)


# --- package.open ----------------------------------------------------------------------------------------------


class OpenPackageParams(Params):
    versions: list[str] = Field(default_factory=lambda: ["baseline", "latest"], min_length=1, max_length=2)

    @field_validator("versions")
    @classmethod
    def known_versions(cls, items: list[str]) -> list[str]:
        for item in items:
            version_spec(item)
        return items


def printed_version(ctx: Ctx, tag: str) -> str | None:
    proc = clients.docker(ctx, "image", "inspect", f"dbt-vdp:{tag}", "--format",
                          '{{index .Config.Labels "org.opencontainers.image.version"}}', check=False)
    return proc.stdout.strip() or None if proc.returncode == 0 else None


@action("package.open", OpenPackageParams, lambda p: "De verzegelde packages openen" if len(p.versions) > 1 else "Het verzegelde package openen")
def open_package(ctx: Ctx, p: OpenPackageParams) -> Result:
    built = built_tags(ctx)
    tags = [resolve(v, ctx.env, built, ctx.settings.baseline_tag) for v in p.versions]
    # `resolve` also serves stored versions; a package that was never built cannot be opened
    tools = [images.list_tools(ctx, f"dbt-vdp:{t}") for t in tags]
    sentences = [docs.package_description(ctx, t, docs.ReadDescriptionParams()) for t in tags]
    printed = [printed_version(ctx, t) or "geen" for t in tags]
    rows = []

    def row(label: str, values: list[str], same: bool | None = None) -> dict[str, Any]:
        out: dict[str, Any] = {"label": label, "left": values[0]}
        if len(values) > 1:
            out.update(right=values[1], same=values[0] == values[1] if same is None else same)
        return out

    rows.append(row("Nummer dat erop gedrukt staat", printed))
    for key, label in (("report_builder", images.PROBES["report_builder"][0]), ("connector", images.PROBES["connector"][0])):
        same = len({t[key] for t in tools}) == 1
        rows.append(row(label, [images.shown(key, t[key], side="left" if i == 0 else "right", same=same) if len(tools) > 1
                                else images.shown(key, t[key]) for i, t in enumerate(tools)], same))
    rows.append(row("Beschrijving van het rapport in de code", [s or "geen" for s in sentences]))
    titles = [f"Package {t}" for t in tags]
    data = {"reasons": rows, "label_title": "In het package", "left_title": titles[0], "tags": tags,
            **({"right_title": titles[1]} if len(tags) > 1 else {})}
    return Result(
        (f"De verzegelde packages van {' en '.join(tags)} geopend." if len(tags) > 1
         else f"Het verzegelde package van {tags[0]} geopend."),
        details="\n".join(f"dbt-vdp:{t}: label {pr}, builder {tl['report_builder']}, connector {tl['connector']}"
                          for t, pr, tl in zip(tags, printed, tools)), data=data)


# --- what demos leave behind -----------------------------------------------------------------------------------


def drop_orphans(api: clients.GitLabApi, tags: set[str]) -> list[str]:
    """Delete the stored packages of versions whose tag is gone (a demo's version). Returns the versions dropped."""
    dropped = []
    for package in registry_packages(api, PACKAGE):
        if package["version"] not in tags:
            api.req("DELETE", api.p(f"/packages/{package['id']}"), ok=(200, 204, 404))
            dropped.append(package["version"])
    return dropped


def restore_env_copy(ctx: Ctx, env: str) -> str:
    """Put the environment's copy in storage back to what the baseline version stored: every file newer than the pair
    (tarball and note) of the baseline version is deleted; with no baseline pair, the copy is deleted. Idempotent."""
    api = clients.GitLabApi(ctx.settings)
    package = next((p for p in registry_packages(api, ENV_PACKAGE) if p["version"] == env), None)
    if package is None:
        return "no copy in storage"
    base = next((v for v in stored_versions(api) if v["version"] == ctx.settings.baseline_tag), None)
    files = files_of(api, package["id"])
    keep: set[int] = set()
    if base is not None:
        same = [f for f in files if f["file_name"] == TARBALL and f["file_sha256"] == base["sha256"]]
        if same:
            tarball = same[-1]
            keep.add(tarball["id"])
            notes = [f for f in files if f["file_name"] == NOTE and f["created_at"] >= tarball["created_at"]]
            if notes:
                keep.add(notes[0]["id"])   # the note written right after that tarball
    if not keep:
        api.req("DELETE", api.p(f"/packages/{package['id']}"), ok=(200, 204, 404))
        return "copy removed"
    removed = [f for f in files if f["id"] not in keep]
    for f in removed:
        api.req("DELETE", api.p(f"/packages/{package['id']}/package_files/{f['id']}"), ok=(200, 204, 404))
    return f"{len(removed)} newer files removed" if removed else "already as at the baseline"
