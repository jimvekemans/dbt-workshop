"""GitLab actions (REST v4, root PAT): branches, patch commits, squash-merge MRs, pipeline/tag waits, manual jobs."""
from __future__ import annotations

import re
import time
import urllib.parse
from typing import Any

import requests

from ..patches import load_patch, resolve_contents
from .base import Action, ActionError, Ctx, Result, register, require

PROTECTED = {   # same rules bootstrap_stack.seed_gitlab_api applies
    "main": {"push_access_level": 40, "merge_access_level": 40, "allow_force_push": True},
    "dev": {"push_access_level": 40, "merge_access_level": 30, "allow_force_push": False},
}


def quote(s: str) -> str:
    return urllib.parse.quote(s, safe="")


class GitLab:
    def __init__(self, ctx: Ctx):
        self.ctx = ctx
        s = ctx.settings
        self.base = f"{s.gitlab_url}/api/v4"
        self.headers = {"PRIVATE-TOKEN": s.gitlab_pat}

    def req(self, method: str, path: str, ok: tuple[int, ...] = (200, 201, 204), **kw) -> requests.Response:
        r = requests.request(method, f"{self.base}{path}", headers=self.headers, timeout=30, **kw)
        if r.status_code not in ok:
            raise ActionError(f"GitLab {method} {path} -> {r.status_code}: {self.ctx.settings.mask(r.text[:300])}")
        return r

    @property
    def pid(self) -> int:
        cache = self.ctx.cache
        if "gl_pid" not in cache:
            cache["gl_pid"] = self.req("GET", f"/projects/{quote(self.ctx.settings.gitlab_project)}").json()["id"]
        return cache["gl_pid"]

    def p(self, path: str) -> str:
        return f"/projects/{self.pid}{path}"

    # --- repository -------------------------------------------------------
    def branch_exists(self, name: str) -> bool:
        return self.req("GET", self.p(f"/repository/branches/{quote(name)}"), ok=(200, 404)).status_code == 200

    def ref_sha(self, ref: str) -> str:
        """Commit of a branch, or of a tag when no such branch exists (tag pipelines: publish_dev_artifacts, tag_main)."""
        r = self.req("GET", self.p(f"/repository/branches/{quote(ref)}"), ok=(200, 404))
        if r.status_code == 200:
            return r.json()["commit"]["id"]
        return self.req("GET", self.p(f"/repository/tags/{quote(ref)}")).json()["commit"]["id"]

    def branch_sha(self, name: str) -> str:
        return self.req("GET", self.p(f"/repository/branches/{quote(name)}")).json()["commit"]["id"]

    def read_file(self, path: str, ref: str) -> str | None:
        r = self.req("GET", self.p(f"/repository/files/{quote(path)}/raw"), ok=(200, 404), params={"ref": ref})
        return r.text if r.status_code == 200 else None

    def delete_branch(self, name: str) -> None:
        self.req("DELETE", self.p(f"/repository/branches/{quote(name)}"), ok=(200, 204, 404))

    def tag_exists(self, tag: str) -> bool:
        return self.req("GET", self.p(f"/repository/tags/{quote(tag)}"), ok=(200, 404)).status_code == 200

    def tag_sha(self, tag: str) -> str | None:
        """Commit a tag points at (annotated tags are peeled by the API), or None."""
        r = self.req("GET", self.p(f"/repository/tags/{quote(tag)}"), ok=(200, 404))
        return r.json()["commit"]["id"] if r.status_code == 200 else None

    def tag_names(self) -> list[str]:
        return [t["name"] for t in self.req("GET", self.p("/repository/tags"), params={"per_page": 100}).json()]

    # --- pipelines --------------------------------------------------------
    def pipeline_for(self, sha: str, ref: str | None = None) -> dict[str, Any] | None:
        """Newest pipeline for a commit. Pass `ref`: a tag pipeline (2026-Q1.0) shares the SHA with the main pipeline."""
        params = {"sha": sha, "order_by": "id", "sort": "desc", "per_page": 1}
        if ref:
            params["ref"] = ref
        r = self.req("GET", self.p("/pipelines"), params=params)
        data = r.json()
        return data[0] if data else None

    def failed_job_tail(self, pipeline_id: int, lines: int = 25) -> str:
        jobs = self.req("GET", self.p(f"/pipelines/{pipeline_id}/jobs"), params={"per_page": 50}).json()
        out = []
        for j in jobs:
            if j["status"] == "failed":
                trace = self.req("GET", self.p(f"/jobs/{j['id']}/trace"), ok=(200, 404)).text
                out.append(f"--- {j['name']} (failed) ---\n" + "\n".join(trace.splitlines()[-lines:]))
        return "\n".join(out)


@register
class GitlabBranch(Action):
    name = "gitlab_branch"

    def render(self, p, ctx):
        require(p, "name", "ref")
        return [f"POST /projects/:id/repository/branches?branch={p['name']}&ref={p['ref']}   # git checkout -b {p['name']} origin/{p['ref']}"]

    def run(self, p, ctx):
        gl = GitLab(ctx)
        if gl.branch_exists(p["name"]):
            if not p.get("recreate", True):
                return Result(output=f"{p['name']} already exists")
            gl.delete_branch(p["name"])
        gl.req("POST", gl.p("/repository/branches"), params={"branch": p["name"], "ref": p["ref"]})
        return Result(output=f"{p['name']} created from {p['ref']}")


@register
class GitlabCommitPatch(Action):
    name = "gitlab_commit_patch"

    def render(self, p, ctx):
        require(p, "patch", "branch")
        patch = load_patch(ctx.settings.patches_dir, p["patch"])
        files = "\n".join(f"  {f}" for f in patch.touched_paths())
        return [f"POST /projects/:id/repository/commits  branch={p['branch']}  \"{patch.message}\"\n{files}"]

    def run(self, p, ctx):
        gl = GitLab(ctx)
        patch = load_patch(ctx.settings.patches_dir, p["patch"])
        branch = p["branch"]
        contents = resolve_contents(patch, lambda path: gl.read_file(path, branch))
        actions = [{"action": "update" if gl.read_file(path, branch) is not None else "create",
                    "file_path": path, "content": text} for path, text in contents.items()]
        r = gl.req("POST", gl.p("/repository/commits"),
                   json={"branch": branch, "commit_message": patch.message, "actions": actions}).json()
        return Result(output=f"{r['short_id']} {patch.message}", data={"sha": r["id"]})


@register
class GitlabMrMerge(Action):
    name = "gitlab_mr_merge"

    def render(self, p, ctx):
        require(p, "source", "target")
        return [f"POST /projects/:id/merge_requests  {p['source']} -> {p['target']}\n"
                f"PUT  /projects/:id/merge_requests/:iid/merge  squash=true   # project squash_option=always"]

    def run(self, p, ctx):
        gl = GitLab(ctx)
        title = p.get("title") or load_title(ctx, p)
        mr = gl.req("POST", gl.p("/merge_requests"), json={
            "source_branch": p["source"], "target_branch": p["target"], "title": title,
            "squash": True, "remove_source_branch": False}).json()
        iid = mr["iid"]
        if p.get("wait_pipeline", True):
            wait_mr_pipeline(gl, ctx, iid, int(p.get("pipeline_timeout", 1500)))
        deadline = time.time() + int(p.get("timeout", 600))
        while time.time() < deadline:
            r = gl.req("PUT", gl.p(f"/merge_requests/{iid}/merge"), ok=(200, 405, 406, 422),
                       json={"squash": True, "squash_commit_message": title, "should_remove_source_branch": False})
            if r.status_code == 200:
                return Result(output=f"MR !{iid} squash-merged into {p['target']}", data={"iid": iid})
            ctx.say(f"  MR !{iid} not mergeable yet ({r.status_code}); waiting for pipeline...")
            ctx.sleep(8)
        raise ActionError(f"MR !{iid} was not mergeable within {p.get('timeout', 600)}s")


def wait_mr_pipeline(gl: GitLab, ctx: Ctx, iid: int, timeout: int) -> None:
    """The MR pipeline (verify_feature_to_dev: dbt build state:modified+) is the point of the MR: merge only when it is green.
    If no pipeline shows up within a minute (no runner / no CI config) the merge proceeds."""
    start = time.time()
    last = None
    while time.time() - start < timeout:
        mr = gl.req("GET", gl.p(f"/merge_requests/{iid}")).json()
        pipe = mr.get("head_pipeline")
        if not pipe:
            if time.time() - start > 60:
                ctx.say(f"  MR !{iid}: no pipeline started, merging without CI")
                return
        else:
            if pipe["status"] != last:
                ctx.say(f"  MR !{iid} pipeline #{pipe['id']}: {pipe['status']}")
                last = pipe["status"]
            if pipe["status"] in ("success", "skipped"):
                return
            if pipe["status"] in ("failed", "canceled"):
                tail = gl.failed_job_tail(pipe["id"])
                if tail:
                    ctx.say(tail)
                raise ActionError(f"MR !{iid} pipeline #{pipe['id']} {pipe['status']} - the feature does not pass CI")
        ctx.sleep(8)
    raise ActionError(f"MR !{iid} pipeline did not finish within {timeout}s")


def load_title(ctx: Ctx, p: dict[str, Any]) -> str:
    if p.get("patch"):
        return load_patch(ctx.settings.patches_dir, p["patch"]).message
    return f"Merge {p['source']} into {p['target']}"


def settled_status(pipeline_status: str, job_statuses: list[str]) -> str:
    """GitLab reports `manual` while only manual jobs wait. For the studio that is 'finished': red if any job failed, else green."""
    if pipeline_status != "manual":
        return pipeline_status
    return "failed" if any(j == "failed" for j in job_statuses) else "success"


@register
class GitlabPipelineWait(Action):
    name = "gitlab_pipeline_wait"

    def render(self, p, ctx):
        if not p.get("tag_of"):
            require(p, "ref")
        what = f"the tag on the head of {p['tag_of']}" if p.get("tag_of") else p["ref"]
        return [f"GET /projects/:id/pipelines?sha=<head of {what}>   # wait for {p.get('expect', 'success')}"]

    def run(self, p, ctx):
        gl = GitLab(ctx)
        expect = p.get("expect", "success")
        if p.get("tag_of"):
            # e.g. publish_dev_artifacts runs on the tag that tag_dev puts on the head of dev: wait for that tag, then its pipeline
            head = gl.branch_sha(p["tag_of"])
            tag_deadline = time.time() + int(p.get("timeout", 900))
            tags: list[str] = []
            while time.time() < tag_deadline and not tags:
                tags = [t["name"] for t in gl.req("GET", gl.p(f"/repository/commits/{head}/refs"), params={"type": "tag"}).json()]
                if not tags:
                    ctx.sleep(8)
            if not tags:
                raise ActionError(f"no tag appeared on the head of {p['tag_of']}")
            p = {**p, "ref": sorted(tags)[-1]}
            ctx.say(f"  waiting for the pipeline of tag {p['ref']}")
        sha = gl.ref_sha(p["ref"])
        deadline = time.time() + int(p.get("timeout", 1200))
        last = None
        while time.time() < deadline:
            pipe = gl.pipeline_for(sha, p["ref"])
            status = pipe["status"] if pipe else "none"
            if pipe and status == "manual":
                # everything automatic has run; only manual jobs (promote_release_to_main, deploy_*) are waiting
                jobs = gl.req("GET", gl.p(f"/pipelines/{pipe['id']}/jobs"), params={"per_page": 100}).json()
                status = settled_status(status, [j["status"] for j in jobs])
            if status != last:
                ctx.say(f"  pipeline on {p['ref']} ({sha[:8]}): {status}")
                last = status
            if status in ("success", "failed", "canceled"):
                tail = gl.failed_job_tail(pipe["id"]) if status == "failed" else ""
                if tail:
                    ctx.say(tail)
                if status != expect:
                    raise ActionError(f"pipeline on {p['ref']} is {status}, expected {expect}")
                return Result(output=f"pipeline {status}", data={"status": status, "id": pipe["id"], "log": tail})
            ctx.sleep(10)
        raise ActionError(f"pipeline on {p['ref']} did not finish in time (last: {last})")


@register
class GitlabTagWait(Action):
    name = "gitlab_tag_wait"

    def render(self, p, ctx):
        require(p, "tag")
        on = f" and points at the head of {p['ref']}" if p.get("ref") else ""
        return [f"GET /projects/:id/repository/tags/{p['tag']}   # created by the tag_dev / tag_main CI job{on}"]

    def run(self, p, ctx):
        gl = GitLab(ctx)
        deadline = time.time() + int(p.get("timeout", 900))
        while time.time() < deadline:
            r = gl.req("GET", gl.p(f"/repository/tags/{quote(p['tag'])}"), ok=(200, 404))
            if not p.get("ref"):
                if r.status_code == 200:
                    return Result(output=f"tag {p['tag']} exists")
            else:
                # a stale tag with the same name must not count: it has to sit on the branch head we are waiting for
                head = gl.branch_sha(p["ref"])
                if r.status_code == 200 and r.json()["commit"]["id"] == head:
                    return Result(output=f"tag {p['tag']} exists")
                # fail fast: CI already tagged the head with something else (e.g. v1.2.0 because dev held other work)
                other = unexpected_head_tag(p["tag"], head_tags(gl, head))
                if other:
                    raise ActionError(
                        f"{p['ref']} was tagged {other}, not {p['tag']}: {p['ref']} already held other work when this "
                        f"scenario started. Press Reset (GitLab back to the baseline) and run the scenario again.")
            ctx.sleep(8)
        raise ActionError(f"tag {p['tag']} did not appear" + (f" on {p['ref']}" if p.get("ref") else "") + " in time")


def head_tags(gl: GitLab, sha: str) -> list[str]:
    return [t["name"] for t in gl.req("GET", gl.p(f"/repository/commits/{sha}/refs"), params={"type": "tag"}).json()]


def _semver(tag: str) -> tuple[int, int, int] | None:
    m = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", tag)
    return (int(m[1]), int(m[2]), int(m[3])) if m else None


def unexpected_head_tag(expected: str, tags: list[str]) -> str | None:
    """A tag CI put on the head *instead of* `expected`: a newer dev version (v1.2.0 while waiting for v1.1.0) or
    another tag of the same release (2026-Q1.1 while waiting for 2026-Q1.0). An older tag (the head has not
    moved yet, e.g. still v1.0.0) is not a reason to stop waiting."""
    exp = _semver(expected)
    if exp:
        newer = [t for t in tags if (_semver(t) or (0, 0, 0)) > exp]
    else:
        series = expected.rsplit(".", 1)[0] + "."          # 2026-Q1.0 -> "2026-Q1."
        newer = [t for t in tags if t.startswith(series) and t != expected]
    return newer[0] if newer else None


@register
class GitlabJobPlay(Action):
    name = "gitlab_job_play"

    def render(self, p, ctx):
        require(p, "ref", "job")
        return [f"POST /projects/:id/jobs/:job_id/play   # manual job {p['job']} on {p['ref']}, then wait for it to finish"]

    def run(self, p, ctx):
        gl = GitLab(ctx)
        deadline = time.time() + int(p.get("timeout", 600))
        job = None
        while time.time() < deadline and job is None:
            pipe = gl.pipeline_for(gl.branch_sha(p["ref"]), p["ref"])
            if pipe:
                jobs = gl.req("GET", gl.p(f"/pipelines/{pipe['id']}/jobs"), params={"per_page": 100}).json()
                job = next((j for j in jobs if j["name"] == p["job"]), None)
            if job is None:
                ctx.sleep(8)
        if job is None:
            raise ActionError(f"manual job {p['job']} not found on {p['ref']}")
        # idempotent: a job that already ran is not played twice (failed ones are retried)
        if job["status"] == "manual":
            job = gl.req("POST", gl.p(f"/jobs/{job['id']}/play")).json()
        elif job["status"] == "failed":
            job = gl.req("POST", gl.p(f"/jobs/{job['id']}/retry")).json()
        else:
            ctx.say(f"  {p['job']} is already {job['status']}")
        while p.get("wait", True) and time.time() < deadline:
            job = gl.req("GET", gl.p(f"/jobs/{job['id']}")).json()
            if job["status"] in ("success", "failed", "canceled"):
                break
            ctx.sleep(8)
        if job["status"] in ("failed", "canceled"):
            trace = gl.req("GET", gl.p(f"/jobs/{job['id']}/trace"), ok=(200, 404)).text
            ctx.say("\n".join(trace.splitlines()[-20:]))
            raise ActionError(f"{p['job']} {job['status']}")
        return Result(output=f"{p['job']} {job['status']}")
