"""GitLab actions: demo branches, committing a prepared change, proposing and accepting it, waiting for the automatic
checks, and playing a manual step. All of them work on project `workshop/dbt-vdp` through the REST API.

Baseline: the first time any of these actions (or the `git` cleaner) touches GitLab, `snapshot()` records the pristine
state in `<data_dir>/git_baseline.json`: the commits of `dev` and `main`, every branch and every tag. Two
`studio-baseline/*` branches keep the two commits reachable, because a force push would otherwise orphan them. The
cleaner returns to exactly that state. This is more robust than deriving it from tags (no assumption about how the
stack was seeded) and cheap; the snapshot is renewed when its commits no longer exist (GitLab was re-provisioned).
"""

from __future__ import annotations

import json
import re
from datetime import datetime
from pathlib import Path
from typing import Any, Literal

from pydantic import ConfigDict, Field, field_validator

from ..names import check_title, column_words, local_time_words, model_title
from ..patches import load_patch, resolve_contents
from . import clients
from .base import Ctx, Params, Result, StepFailed, action
from .clients import GitLabApi
from .deploy import version_key

BRANCH_WORDS = {"dev": "the shared work in progress", "main": "production"}


def branch_words(name: str) -> str:
    """A branch in the words of the audience (the exact name goes to the technical details)."""
    return "the copy for the release" if RELEASE_BRANCH.fullmatch(name) else BRANCH_WORDS.get(name, "a copy of the shared work")

JOB_WORDS = {   # one label per kind of job, the same in progress lines, the card and every summary
    "verify_feature_to_dev": "Trying the change in a private copy",
    "verify_release": "Checking the whole release",
    "tag_dev": "Numbering the version",
    "tag_main": "Numbering the version",
    "build_image": "Packaging the version",
    "deploy_dev": "Putting the version live in development",
    "deploy_val": "Putting the version live in validation",
    "deploy_prd": "Putting the version live in production",
}
STAGES = ("verify", "tag", "build", "deploy")   # the order of `stages:` in .gitlab-ci.yml
KEEP_ALIVE = "studio-baseline/"  # branches that keep the baseline commits reachable after dev / main were rewritten
TERMINAL = {"success", "failed", "canceled", "skipped"}
RELEASE_BRANCH = re.compile(r"release/\d{4}-Q[1-4]")
TARGET = re.compile(r"(mr|ref|tag):(.+)")


def job_word(name: str) -> str:
    return JOB_WORDS.get(name, "A step of the automatic checks")


def ordered_jobs(jobs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The jobs in the order they run: stage order, then start time (jobs that have not started, last, by id)."""
    def key(job: dict[str, Any]) -> tuple:
        stage = job.get("stage")
        return (STAGES.index(stage) if stage in STAGES else len(STAGES), job.get("started_at") or "\uffff", job.get("id") or 0)

    return sorted(jobs, key=key)


# --- the data check behind a failed verify job ------------------------------------------------------------------

_LOG_PREFIX = re.compile(r"^\d{4}-\d\d-\d\dT[\d:.]+Z\s\d\d[OE][+ ]?")   # GitLab's `<time> 01O ` in front of every line
_ANSI = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
_DBT_TIME = re.compile(r"^\d\d:\d\d:\d\d(?:\.\d+)?\s+")
# The patterns of airflow/utils/dbt_failure_report.py (dbt 2 text output).
_FAILED_TEST = re.compile(r"^Failed\s+(?:unit_)?test\s+(?P<name>\S+)")
_TEST_DETAIL = re.compile(r"^Test failed \((?P<rows>\d+) failed row\(s\)\):\s*(?P<name>\S+)\s*$")
_CHECKS_OF_JOB: dict[int, list[dict[str, Any]]] = {}   # a finished job's log never changes: read it once


_FAILED_MODEL = re.compile(r"^Failed\s+model\s+(?P<name>\S+)")
_MODEL_ERROR = re.compile(r"Database Error in model (?P<name>\S+)")
_DUPLICATE_KEY = re.compile(r"Duplicate entry '[^']*' for key '(?P<key>[^']*)'")
_KEY_COLUMN = re.compile(r"(?:column|key)\s+'?`?(?P<col>[a-z][a-z0-9_]*)`?'?", re.IGNORECASE)
# The unique key of the main models in the words of the audience, for when the log names no column.
MODEL_KEYS = {"fct_olist_orders": "order number", "dim_olist_customers": "customer number"}


def plain_reason(error: str, model: str = "") -> str:
    """The common database errors in plain words (no final full stop)."""
    low = error.lower()
    if "duplicate entry" in low or "error 1062" in low:
        key = _DUPLICATE_KEY.search(error)
        column = key["key"] if key and key["key"].upper() != "PRIMARY" and re.fullmatch(r"[a-z][a-z0-9_]*", key["key"]) else None
        what = column_words(column) if column else next((w for m, w in MODEL_KEYS.items() if model.endswith(m)), None)
        return f"the same {what} would be stored more than once" if what else "the same row would be stored more than once"
    if re.search(r"error 10(46|49|51|54|146)|doesn't exist|does not exist|unknown column|unknown table|not found", low):
        return "it needs something that does not exist"
    if re.search(r"error 10(44|45|142|143)|denied|not allowed|permission", low):
        return "the robot is not allowed to do this"
    return "the database reported an error"


def failed_checks_in(trace: str) -> list[dict[str, Any]]:
    """What went wrong in a dbt job log, as `[{kind, text, rows, details}]`: a failed data check (`kind` "check",
    `rows` = failed rows or None, `details` = its technical name) or a table that could not be built (`kind`
    "table", `rows` None, `details` = the raw database error)."""
    order: list[str] = []
    rows: dict[str, int] = {}
    tables: list[str] = []
    errors: dict[str, list[str]] = {}
    current: list[str] | None = None
    for raw in trace.splitlines():
        line = _DBT_TIME.sub("", _ANSI.sub("", _LOG_PREFIX.sub("", raw)).strip())
        if found := _FAILED_TEST.match(line):
            current = None
            if found["name"] not in order:
                order.append(found["name"])
        elif found := _TEST_DETAIL.match(line):
            current = None
            if found["name"] not in order:
                order.append(found["name"])
            rows[found["name"]] = int(found["rows"])
        elif found := _FAILED_MODEL.match(line):
            name = found["name"].split(".")[-1]
            if name not in tables:
                tables.append(name)
        elif found := _MODEL_ERROR.search(line):
            current = errors.setdefault(found["name"].split(".")[-1], [])
            current.append(line)
        elif current is not None and line and not line.startswith(("[error]", "=")) and len(current) < 4:
            current.append(line)
        else:
            current = None
    out: list[dict[str, Any]] = [
        {"kind": "check", "text": check_title(name), "rows": rows.get(name), "details": name} for name in order]
    for name in tables:
        error = "\n".join(errors.get(name, []))
        out.append({"kind": "table", "text": f"{model_title(name) or check_title(name)} could not be built: "
                                             f"{plain_reason(error, name)}.", "rows": None, "details": error or name})
    return out


def failures(api: GitLabApi, jobs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """What went wrong in the failed verify jobs (data checks and tables that could not be built). Never raises: the card must show without this line."""
    out: list[dict[str, Any]] = []
    for job in ordered_jobs(jobs):
        if job["status"] != "failed" or not job["name"].startswith("verify"):
            continue
        if job.get("id") not in _CHECKS_OF_JOB:
            try:
                trace = api.req("GET", api.p(f"/jobs/{job['id']}/trace"), ok=(200, 404)).text
            except Exception:  # noqa: BLE001
                return out
            if len(_CHECKS_OF_JOB) > 64:
                _CHECKS_OF_JOB.clear()
            _CHECKS_OF_JOB[job.get("id")] = failed_checks_in(trace)
        out += [c for c in _CHECKS_OF_JOB[job.get("id")] if c not in out]
    return out


# --- what a rehearsal covered ------------------------------------------------------------------------------------

REHEARSAL_JOB = "verify_feature_to_dev"   # the only job that rehearses just the changed tables (state:modified+)
_NODE_LINE = re.compile(
    r"^(?P<status>Succeeded|Passed|Failed|Errored|Warned|Skipped)\b.*?\b(?P<kind>model|test|unit_test|seed|snapshot)\b")
_TRACES: dict[int, str] = {}   # a finished job's log never changes: read it once


def job_trace(api: GitLabApi, job: dict[str, Any]) -> str:
    """The log of a finished job, without GitLab's time prefix and colours (empty when it cannot be read)."""
    if job.get("id") not in _TRACES:
        try:
            raw = api.req("GET", api.p(f"/jobs/{job['id']}/trace"), ok=(200, 404)).text
        except Exception:  # noqa: BLE001 - the card must show without these lines
            return ""
        if len(_TRACES) > 64:
            _TRACES.clear()
        _TRACES[job["id"]] = "\n".join(_DBT_TIME.sub("", _ANSI.sub("", _LOG_PREFIX.sub("", ln)).strip()) for ln in raw.splitlines())
    return _TRACES[job["id"]]


_BUILD_END = re.compile(r"^Finished '(?:build|run|test|seed|snapshot)'")
_PROCESSED = re.compile(r"^Processed:\s*(?P<parts>.+)$")
_COUNT = re.compile(r"(?P<n>\d+)\s+(?P<what>models?|seeds?|snapshots?|(?:unit )?tests?)\b")


def nodes_in(trace: str) -> dict[str, int]:
    """How many tables and checks dbt ran in this log (`{"tables": n, "checks": n}`). dbt's closing line of the build
    says it (`Processed: 1 hook | 4 models | 3 tests`); without that line the result lines are counted."""
    after_build = False   # the log also holds the earlier `dbt compile` of the whole project: its Processed line is not it
    for line in trace.splitlines():
        text = line.strip()
        if _BUILD_END.match(text):
            after_build = True
        elif after_build and (found := _PROCESSED.match(text)):
            tables = checks = 0
            for count in _COUNT.finditer(found["parts"]):
                if count["what"].startswith(("model", "seed", "snapshot")):
                    tables += int(count["n"])
                else:
                    checks += int(count["n"])
            return {"tables": tables, "checks": checks}
    tables = checks = 0
    for line in trace.splitlines():
        found = _NODE_LINE.match(line.strip())
        if not found or found["status"] == "Skipped":
            continue
        if found["kind"] in ("model", "seed", "snapshot"):
            tables += 1
        else:
            checks += 1
    return {"tables": tables, "checks": checks}


PROJECT_MANIFESTS = ("target", "target_drift_audit")   # the last run's, and the drift audit's (it sees the whole project)


def tables_in_project(settings: Any) -> int | None:
    """The number of tables of the deployed project: the models in the manifests development keeps. A night that left
    tables out writes a smaller manifest in `target`, so the largest count of the manifests is the project."""
    counts = []
    for folder in PROJECT_MANIFESTS:
        path = Path(settings.state_dir) / "dev" / "mnt_dbt" / folder / "manifest.json"
        try:
            nodes = json.loads(path.read_text()).get("nodes", {})
        except (OSError, ValueError):
            continue
        counts.append(sum(1 for n in nodes.values() if n.get("resource_type") == "model"))
    return max(counts, default=0) or None


_MODEL_RESULT = re.compile(r"^(?:Succeeded|Failed|Errored|Warned)\s+(?:model|seed|snapshot)\s+(?P<name>\S+)")
_BUILD_TIME = re.compile(r"\[\s*(?P<n>\d+(?:\.\d+)?)\s*(?P<unit>ms|s|m)\s*\]\s*$")
MAX_NAMES = 6


def seconds_in(trace: str) -> float | None:
    """dbt's own build time: the number in the brackets of the closing line `Finished 'build' ... [1.9s]`. The compile
    before it has such a line too; the build's is the one wanted. None when the log has no closing line."""
    for line in trace.splitlines():
        text = line.strip()
        if _BUILD_END.match(text) and (found := _BUILD_TIME.search(text)) and not text.startswith("Finished 'compile'"):
            value = float(found["n"])
            return round(value / 1000 if found["unit"] == "ms" else value * 60 if found["unit"] == "m" else value, 2)
    return None


def plain_table(alias: str) -> str:
    """The plain name of a built table from its alias in the log (`<schema>_<model>` in a private copy)."""
    if title := model_title(alias):
        return title
    own = re.search(r"(?:^|_)((?:stg|int|dim|fct|ref|mart)_.+)$", alias)
    return (own and model_title(own[1])) or alias.replace("_", " ").capitalize() + " table"


def names_in(trace: str) -> list[str]:
    """The plain names of the tables dbt built in this log (in order, each once): at most 6, then one entry
    "and N more"."""
    found: list[str] = []
    for line in trace.splitlines():
        if (hit := _MODEL_RESULT.match(line.strip())):
            schema, _, table = hit["name"].strip("`'\"").rpartition(".")
            table = table.removeprefix(schema + "_") if schema else table   # a private copy prefixes every table with its name
            plain = plain_table(table)
            if plain not in found:
                found.append(plain)
    if len(found) > MAX_NAMES:
        return found[:MAX_NAMES] + [f"and {len(found) - MAX_NAMES} more"]
    return found


def rehearsed(settings: Any, api: GitLabApi, jobs: list[dict[str, Any]]) -> dict[str, Any] | None:
    """`{tables, checks, tables_total, seconds, names}` of a finished rehearsal job, read from its log; None when there
    is no such job yet, or its log says nothing about the build. `seconds` is dbt's own build time (a number, None when
    the log has no closing line), `names` the plain names of the rehearsed tables."""
    job = next((j for j in jobs if j["name"] == REHEARSAL_JOB and j["status"] in ("success", "failed")), None)
    if job is None:
        return None
    trace = job_trace(api, job)
    counted = nodes_in(trace)
    total = tables_in_project(settings)
    if not counted["tables"] or total is None:
        return None
    return {**counted, "tables_total": total, "seconds": seconds_in(trace), "names": names_in(trace)}


def duration_words(seconds: float) -> str:
    whole = max(1, round(seconds))
    if whole >= 120:
        minutes, rest = divmod(whole, 60)
        return f"{minutes} minutes" + (f" and {rest} second{'' if rest == 1 else 's'}" if rest else "")
    return f"{whole} second{'' if whole == 1 else 's'}"


def rehearsed_words(found: dict[str, Any]) -> str:
    """"1 of 28 tables and 0 data checks in 2 seconds": the same words on the card and in the step's summary."""
    checks = found["checks"]
    text = f"{found['tables']} of {found['tables_total']} tables and {checks} data check{'' if checks == 1 else 's'}"
    return text + (f" in {duration_words(found['seconds'])}" if found.get("seconds") else "")


def demo_branch(name: str) -> str:
    if not name.startswith("demo/"):
        raise ValueError("demo branches must start with demo/")
    return name


def pipeline_target(spec: str) -> str:
    if not TARGET.fullmatch(spec):
        raise ValueError("`for` is mr:<source branch>, ref:<branch or tag> or tag:<regex>")
    return spec


class BranchParams(Params):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    name: str
    from_: str = Field("dev", alias="from")

    @field_validator("name")
    @classmethod
    def only_demo_branches(cls, name: str) -> str:
        """Demo branches, and the release branch of the release demo (it is removed by the clean-up too)."""
        return name if RELEASE_BRANCH.fullmatch(name) else demo_branch(name)


class CommitParams(Params):
    branch: str
    patch: str

    @field_validator("branch")
    @classmethod
    def only_demo_branches(cls, name: str) -> str:
        return demo_branch(name)


class MergeRequestParams(Params):
    source: str
    target: str
    title: str


class MergeParams(Params):
    source: str
    timeout: int = 600


class WaitPipelineParams(Params):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    for_: str = Field(alias="for")
    expect: Literal["success", "failed"] = "success"
    timeout: int = 1200
    report_rehearsed: bool = False   # say in the summary how many tables and checks the rehearsal covered

    @field_validator("for_")
    @classmethod
    def known_target(cls, spec: str) -> str:
        return pipeline_target(spec)


class PlayJobParams(Params):
    model_config = ConfigDict(extra="forbid", populate_by_name=True)

    job: str
    for_: str = Field(alias="for")
    timeout: int = 900

    @field_validator("for_")
    @classmethod
    def known_target(cls, spec: str) -> str:
        return pipeline_target(spec)


# --- the pristine state ---------------------------------------------------------------------------


def _baseline_file(settings: Any) -> Path:
    return Path(settings.data_dir) / "git_baseline.json"


def snapshot(settings: Any, api: GitLabApi) -> dict[str, Any]:
    """The recorded pristine state, taken now if there is none (or it is stale)."""
    path = _baseline_file(settings)
    if path.exists():
        saved = json.loads(path.read_text())
        if api.maybe(f"/repository/commits/{saved['dev']}") and api.maybe(f"/repository/commits/{saved['main']}"):
            return saved
    state = {
        "dev": api.branch_sha("dev"),
        "main": api.branch_sha("main"),
        "branches": api.branch_names(),
        "tags": api.tag_names(),
    }
    for name in ("dev", "main"):
        if api.branch_sha(f"{KEEP_ALIVE}{name}") is None:
            api.req("POST", api.p("/repository/branches"), params={"branch": f"{KEEP_ALIVE}{name}", "ref": state[name]})
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state, indent=1))
    return state


def recorded_tags(settings: Any) -> list[str]:
    """Tags of the recorded pristine state; empty before the first snapshot. A file read only, for polling evidence."""
    path = _baseline_file(settings)
    return json.loads(path.read_text())["tags"] if path.exists() else []


def connect(ctx: Ctx) -> tuple[GitLabApi, dict[str, Any]]:
    """The API client, with the pristine state recorded before anything is changed."""
    api = GitLabApi(ctx.settings)
    return api, snapshot(ctx.settings, api)


# --- branch, commit, proposal ---------------------------------------------------------------------


@action("gitlab.branch", BranchParams,
        lambda p: "Making a copy for the release" if RELEASE_BRANCH.fullmatch(p.name) else
        f"Making a copy of {branch_words(p.from_)}")
def branch(ctx: Ctx, p: BranchParams) -> Result:
    api, _ = connect(ctx)
    api.delete_branch(p.name)  # a branch left by an earlier run of the same demo starts over
    api.req("POST", api.p("/repository/branches"), params={"branch": p.name, "ref": p.from_})
    return Result(
        f"Made a copy for the release, taken from {branch_words(p.from_)}."
        if RELEASE_BRANCH.fullmatch(p.name)
        else f"Made a copy of {branch_words(p.from_)} to work on.",
        details=f"branch {p.name} created from {p.from_}",
        touched={"git"},
    )


@action("gitlab.commit", CommitParams, lambda p: "Saving the prepared code change")
def commit(ctx: Ctx, p: CommitParams) -> Result:
    api, _ = connect(ctx)
    patch = load_patch(ctx.settings.patches_dir, p.patch)
    contents = resolve_contents(patch, lambda path: api.read_file(path, p.branch))
    actions = [
        {
            "action": "update" if api.read_file(path, p.branch) is not None else "create",
            "file_path": path,
            "content": text,
        }
        for path, text in contents.items()
    ]
    done = api.req(
        "POST",
        api.p("/repository/commits"),
        json={"branch": p.branch, "commit_message": patch.message, "actions": actions},
    ).json()
    return Result(
        f"Saved the change: {patch.title}.",
        details=f"{done['short_id']} {patch.message}\n" + "\n".join(contents),
        touched={"git"},
        data={"sha": done["id"]},
    )


@action("gitlab.merge_request", MergeRequestParams, lambda p: "Proposing the change for review")
def merge_request(ctx: Ctx, p: MergeRequestParams) -> Result:
    api, _ = connect(ctx)
    open_ones = api.get("/merge_requests", source_branch=p.source, target_branch=p.target, state="opened")
    mr = (
        open_ones[0]
        if open_ones
        else api.req(
            "POST",
            api.p("/merge_requests"),
            json={
                "source_branch": p.source,
                "target_branch": p.target,
                "title": p.title,
                "squash": True,
                "remove_source_branch": False,
            },
        ).json()
    )
    return Result(
        f"Proposed the change '{p.title}' for review.",
        details=f"merge request !{mr['iid']}: {p.source} -> {p.target}",
        touched={"git"},
        data={"iid": mr["iid"]},
    )


# --- the automatic checks -------------------------------------------------------------------------


def is_current(pipeline: dict[str, Any], mr: dict[str, Any]) -> bool:
    """Is `pipeline` the one of the newest commit of the proposed change (when GitLab says which commits)?"""
    return not (pipeline.get("sha") and mr.get("sha")) or pipeline["sha"] == mr["sha"]


def find_pipeline(api: GitLabApi, spec: str, baseline_tags: list[str]) -> dict[str, Any] | None:
    """The pipeline a `for:` target stands for, or None while it has not been created yet."""
    kind, value = TARGET.fullmatch(spec).groups()
    if kind == "mr":
        found = api.get("/merge_requests", source_branch=value, order_by="created_at", sort="desc")
        if not found:
            return None
        mr = api.get(f"/merge_requests/{found[0]['iid']}")
        head = mr.get("head_pipeline")
        # Right after a new commit GitLab still reports the pipeline of the previous one for a moment, and the
        # proposal's own `sha` lags in the same way: the branch itself is the only thing that is never behind.
        newest = api.branch_sha(value)
        return head if head and is_current(head, {**mr, "sha": newest or mr.get("sha")}) else None
    if kind == "ref":
        sha = api.branch_sha(value) or api.tag_sha(value)
        return api.pipeline_for(sha, value) if sha else None
    fresh = [t for t in api.tag_names() if re.search(value, t) and t not in baseline_tags]
    if not fresh:
        return None
    newest = max(fresh, key=lambda t: version_key(t) or (9, ()))
    return api.pipeline_for(api.tag_sha(newest), newest)


def settled(api: GitLabApi, pipeline: dict[str, Any]) -> str:
    """GitLab says `manual` while only manual jobs wait: for the studio that is finished, red if a job failed."""
    if pipeline["status"] != "manual":
        return pipeline["status"]
    return "failed" if any(j["status"] == "failed" for j in api.jobs(pipeline["id"])) else "success"


def delivery_words(jobs: list[dict[str, Any]]) -> str | None:
    """"The version was numbered, packaged and put live in development." for a pipeline that numbers and delivers a
    version (tag, build and deploy jobs, no verify job) and succeeded; None for any other pipeline."""
    names = {j["name"]: j["status"] for j in jobs}
    live = [n.split("_", 1)[1] for n, status in names.items() if n.startswith("deploy_") and status == "success"]
    if any(n.startswith("verify") for n in names) or not live:
        return None
    did = []
    if any(n.startswith("tag_") and status == "success" for n, status in names.items()):
        did.append("numbered")
    if names.get("build_image") == "success":
        did.append("packaged")
    did.append("put live in " + clients.ENV_WORDS[live[-1]])
    return "The version was " + (", ".join(did[:-1]) + " and " + did[-1] if len(did) > 1 else did[0]) + "."


def pipeline_progress(api: GitLabApi, pipeline: dict[str, Any] | None) -> str:
    if pipeline is None:
        return "Waiting for the automatic checks to start"
    running = [j["name"] for j in api.jobs(pipeline["id"]) if j["status"] == "running"]
    return (
        f"Automatic checks: {job_word(running[0]).lower()}"
        if running
        else "Automatic checks: waiting for the next step"
    )


@action("gitlab.wait_pipeline", WaitPipelineParams, lambda p: "Waiting for the automatic checks")
def wait_pipeline(ctx: Ctx, p: WaitPipelineParams) -> Result:
    api, base = connect(ctx)

    def probe() -> tuple[str, dict[str, Any]] | None:
        pipeline = find_pipeline(api, p.for_, base["tags"])
        ctx.progress(pipeline_progress(api, pipeline))
        status = settled(api, pipeline) if pipeline else "none"
        return (status, pipeline) if status in TERMINAL and pipeline else None

    status, pipeline = ctx.wait_until(probe, p.timeout, every=8, waiting="the automatic checks did not finish")
    outcome = "success" if status in ("success", "skipped") else "failed"
    tail = api.failed_job_tail(pipeline["id"]) if outcome == "failed" else ""
    jobs = ordered_jobs(api.jobs(pipeline["id"]))
    failed_jobs = [job_word(j["name"]) for j in jobs if j["status"] == "failed"]
    found_failures = failures(api, jobs) if outcome == "failed" else []
    what = "automatic rehearsal" if TARGET.fullmatch(p.for_).group(1) == "mr" else "automatic checks"
    details = f"pipeline {pipeline['id']} on {pipeline['ref']}: {status}\n{tail}".strip()
    if outcome != p.expect:
        if outcome == "failed":
            raise StepFailed(
                f"The {what} failed" + (f" at: {failed_jobs[0].lower()}." if failed_jobs else "."), details
            )
        raise StepFailed(f"The {what} were expected to fail but passed." if what.endswith("checks")
                         else f"The {what} was expected to fail but passed.", details)
    summary = (
        (delivery_words(jobs) or f"The {what} passed.")
        if outcome == "success"
        else f"The {what} failed, as expected" + (f": {failed_jobs[0].lower()}." if failed_jobs else ".")
    )
    data: dict[str, Any] = {"status": status, "failures": found_failures}
    if found := rehearsed(ctx.settings, api, jobs):
        data["rehearsed"] = found
        if p.report_rehearsed:
            summary += f" It rehearsed {rehearsed_words(found)}."
    if job := next((j for j in jobs if j["name"] == REHEARSAL_JOB and j["status"] in ("success", "failed")), None):
        details += "\n--- the last lines of the rehearsal ---\n" + clients.mask(
            ctx.settings, "\n".join(job_trace(api, job).splitlines()[-40:]))
    return Result(summary, details=details, data=data)


def versions_touched(target: str) -> set[str]:
    """Accepting a change into dev produces a version tag that CI deploys to dev; into main, one that goes to val."""
    return {"version:dev"} if target == "dev" else {"version:val"} if target == "main" else set()


@action("gitlab.merge", MergeParams, lambda p: "Accepting the proposed change")
def merge(ctx: Ctx, p: MergeParams) -> Result:
    api, _ = connect(ctx)
    found = api.get("/merge_requests", source_branch=p.source, state="opened")
    if not found:
        raise StepFailed(
            "There is no proposed change waiting to be accepted.", f"no open merge request from {p.source}"
        )
    mr = found[0]

    def accepted() -> bool:
        current = api.get(f"/merge_requests/{mr['iid']}")
        pipeline = current.get("head_pipeline")
        if pipeline and not is_current(pipeline, current):
            ctx.progress("Waiting for the automatic checks of the change to start")
            return False
        if pipeline:   # the change is only accepted once its automatic checks have finished, and passed
            status = settled(api, pipeline)
            if status in ("failed", "canceled"):
                raise StepFailed(
                    "The automatic checks of this change failed, so it cannot be accepted.",
                    f"merge request !{mr['iid']}: pipeline {pipeline['id']} is {status}",
                )
            if status not in ("success", "skipped"):
                ctx.progress("Waiting for the automatic checks of the change to finish")
                return False
        r = api.req(
            "PUT",
            api.p(f"/merge_requests/{mr['iid']}/merge"),
            ok=(200, 405, 406, 422),
            json={"squash": True, "squash_commit_message": mr["title"], "should_remove_source_branch": False},
        )
        ctx.progress("Waiting until the change may be accepted")
        return r.status_code == 200

    target = mr["target_branch"]
    try:
        ctx.wait_until(accepted, p.timeout, every=8, waiting="the change could not be accepted")
    except StepFailed as e:   # the change may have gone through before the wait gave up
        e.touched |= {"git", *versions_touched(target)}
        raise
    return Result(
        f"The change was accepted into {branch_words(target)}.",
        details=f"merge request !{mr['iid']} merged: {p.source} -> {target}",
        touched={"git", *versions_touched(target)},
    )


def find_job(api: GitLabApi, base: dict[str, Any], spec: str, job: str) -> dict[str, Any] | None:
    pipeline = find_pipeline(api, spec, base["tags"])
    return next((j for j in api.jobs(pipeline["id"]) if j["name"] == job), None) if pipeline else None


def job_versions(job: str) -> set[str]:
    deploy = re.fullmatch(r"deploy_(dev|val|prd)", job)
    return {f"version:{deploy[1]}"} if deploy else {"version:val"}


def approval_lines(job: dict[str, Any]) -> list[str]:
    """Who pressed the button of a manual job and when, as two plain lines (GitLab keeps both with the job). The time is
    local time, the way people say it: "15 November, 18:06"."""
    lines = []
    if name := (job.get("user") or {}).get("name"):
        lines.append(f"Approved by {name}")
    if when := local_time_words(job.get("started_at")):
        lines.append(f"Approved on {when}")
    return lines


@action("gitlab.play_job", PlayJobParams, lambda p: f"Starting: {job_word(p.job).lower()}")
def play_job(ctx: Ctx, p: PlayJobParams) -> Result:
    api, base = connect(ctx)
    job = ctx.wait_until(
        lambda: find_job(api, base, p.for_, p.job), p.timeout, every=8, waiting=f"{p.job} never appeared"
    )
    owed = {"git", *job_versions(p.job)}   # from the moment the button may be pressed, the version has to be undone

    def finished() -> dict[str, Any] | None:
        current = api.req("GET", api.p(f"/jobs/{job['id']}")).json()
        ctx.progress(f"{job_word(p.job)}: {current['status']}")
        return current if current["status"] in ("success", "failed", "canceled") else None

    try:
        if job["status"] in ("manual", "failed"):  # a job that already ran is not played twice; a failed one is retried
            job = api.req("POST", api.p(f"/jobs/{job['id']}/{'play' if job['status'] == 'manual' else 'retry'}")).json()
        done = ctx.wait_until(finished, p.timeout, every=8, waiting=f"{p.job} did not finish")
    except Exception as e:  # noqa: BLE001 - a failed, timed-out or cancelled wait still leaves the job running or done
        e.touched = set(getattr(e, "touched", ())) | owed
        raise
    if done["status"] != "success":
        trace = api.req("GET", api.p(f"/jobs/{job['id']}/trace"), ok=(200, 404)).text
        raise StepFailed(
            f"{job_word(p.job)} did not succeed.",
            clients.mask(ctx.settings, "\n".join(trace.splitlines()[-20:])),
            touched={"git", *job_versions(p.job)},
        )
    who = approval_lines(done)
    return Result(
        f"{job_word(p.job)}: done.",
        details=f"job {p.job} ({job['id']}) {done['status']}",
        touched={"git", *job_versions(p.job)},
        data={"reasons": who, "approved_by": (done.get("user") or {}).get("name"), "approved_at": done.get("started_at")},
    )
