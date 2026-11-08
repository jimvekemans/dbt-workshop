"""Scenario preconditions (`requires:` in a scenario YAML). They run in Live mode before the injection, read-only,
and stop it with a clear message when the stack is not in the state the scenario was written for - instead of a
15-minute wait for a tag that will never appear (dev already held other work) or a pipeline that cannot go red.

Every check takes an optional `hint`: the sentence the presenter sees ("press Reset first", "run E1's injection first")."""
from __future__ import annotations

from .base import Action, PreconditionFailed, Result, register, require
from .gitlab import GitLab
from .sqlops import ENV_DB, connect

BASELINE = {"dev": "v1.0.0", "main": "2026_DROP1.0"}


def _fail(p, detail: str) -> PreconditionFailed:
    return PreconditionFailed(f"{p['hint']} ({detail})" if p.get("hint") else detail)


@register
class CheckGitlabBaseline(Action):
    """dev and main sit exactly on the baseline tags and no other tag exists - i.e. Reset ran and nothing merged since.
    H0 and E1 wait for the tags CI creates next (v1.1.0, v1.2.0, 2026-Q1.0); on a used repo those names are taken."""
    name = "check_gitlab_baseline"

    def render(self, p, ctx):
        return ["# check: dev = v1.0.0, main = 2026_DROP1.0 and no other tags (GitLab at the baseline, i.e. after Reset)"]

    def run(self, p, ctx):
        gl = GitLab(ctx)
        problems = []
        for branch, tag in BASELINE.items():
            if gl.branch_sha(branch) != gl.tag_sha(tag):
                problems.append(f"{branch} has moved past {tag}")
        extra = sorted(t for t in gl.tag_names() if t not in BASELINE.values())
        if extra:
            problems.append("tags " + ", ".join(extra[:4]) + (" ..." if len(extra) > 4 else "") + " exist")
        if problems:
            raise _fail(p, "; ".join(problems))
        return Result(output="GitLab is at the baseline")


@register
class CheckGitlabFile(Action):
    """A file exists (or not) on a branch, optionally without some text - e.g. 'Domain B is merged into dev' or
    'the dedup fix is not in dev yet'."""
    name = "check_gitlab_file"

    @staticmethod
    def _want(p) -> bool | None:
        """exists: true/false; with only `not_contains` a missing file is fine (another check covers presence)."""
        return p.get("exists", None if p.get("not_contains") else True)

    def render(self, p, ctx):
        require(p, "path")
        ref = p.get("ref", "dev")
        want = self._want(p)
        if p.get("not_contains"):
            state = ("exists and " if want else "") + f"does not contain {p['not_contains']!r}"
        else:
            state = "exists" if want else "does not exist"
        return [f"# check: {p['path']} on {ref} {state}"]

    def run(self, p, ctx):
        ref = p.get("ref", "dev")
        content = GitLab(ctx).read_file(p["path"], ref)
        want = self._want(p)
        if want is True and content is None:
            raise _fail(p, f"{p['path']} is not on {ref}")
        if want is False and content is not None:
            raise _fail(p, f"{p['path']} is already on {ref}")
        if content is not None and p.get("not_contains") and p["not_contains"] in content:
            raise _fail(p, f"{p['path']} on {ref} already contains {p['not_contains']!r}")
        return Result(output=f"{p['path']} ok on {ref}")


@register
class CheckSql(Action):
    """A scalar query returns the expected value, e.g. PRD has no reviews yet."""
    name = "check_sql"

    def render(self, p, ctx):
        require(p, "env", "statement", "expect")
        return [f"# check: USE {ENV_DB[p['env']]}; {p['statement']}   -> expect {p['expect']!r}"]

    def run(self, p, ctx):
        conn = connect(ctx, ENV_DB[p["env"]])
        try:
            with conn.cursor() as cur:
                cur.execute(p["statement"])
                row = cur.fetchone()
        finally:
            conn.close()
        got = row[0] if row else None
        if str(got) != str(p["expect"]):
            raise _fail(p, f"{p['statement']} returned {got!r}, expected {p['expect']!r}")
        return Result(output=f"{p['statement']} = {got!r}")
