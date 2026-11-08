"""git CLI scripts run in a temp clone: exactly the commands from the dbt-release-flow skill, output streamed to the UI.

  revert_on_release : git revert <tag> on release/<q>; resolves the guaranteed conflict with a resolution patch set
  promote_release   : merge-base reset + --no-ff merge + force push to main (what promote_release_to_main does)
  reset_branches    : force dev/main back to the baseline tags and drop leftovers (used by live Reset)
"""
from __future__ import annotations

import os
import tempfile
from pathlib import Path
from typing import Any

from ..patches import apply_to_tree, load_patch
from .base import Action, ActionError, Ctx, Result, register, require
from .gitlab import PROTECTED, GitLab, quote

GIT_ENV = {"GIT_TERMINAL_PROMPT": "0", "GIT_EDITOR": "true",
           "GIT_AUTHOR_NAME": "Workshop Studio", "GIT_AUTHOR_EMAIL": "studio@workshop.local",
           "GIT_COMMITTER_NAME": "Workshop Studio", "GIT_COMMITTER_EMAIL": "studio@workshop.local"}


def remote_url(ctx: Ctx, override: str | None = None) -> str:
    if override:
        return override
    s = ctx.settings
    scheme, rest = s.gitlab_url.split("://", 1)
    return f"{scheme}://root:{s.gitlab_pat}@{rest}/{s.gitlab_project}.git"


def git(ctx: Ctx, repo: str, *args: str, check: bool = True):
    return ctx.sh(["git", *args], cwd=repo, check=check, env=GIT_ENV)


def clone(ctx: Ctx, tmp: str, url: str, branch: str) -> str:
    repo = os.path.join(tmp, "repo")
    ctx.sh(["git", "clone", "--quiet", "-b", branch, url, repo], env=GIT_ENV)
    git(ctx, repo, "fetch", "--quiet", "--tags", "origin")
    return repo


def revert_on_release(ctx: Ctx, *, branch: str, tag: str, resolution: str, url: str | None = None) -> str:
    with tempfile.TemporaryDirectory() as tmp:
        repo = clone(ctx, tmp, remote_url(ctx, url), branch)
        # GitLab squash-merges leave a merge commit on dev (squash commit + merge commit); the tag then points at that
        # merge, and reverting a merge needs `-m 1` (diff against the first parent = the whole feature).
        parents = git(ctx, repo, "rev-list", "--parents", "-n", "1", tag).stdout.split()[1:]
        mainline = ["-m", "1"] if len(parents) > 1 else []
        proc = git(ctx, repo, "revert", "--no-edit", *mainline, tag, check=False)
        if proc.returncode == 0:
            ctx.say("revert applied cleanly (no conflict)")
        else:
            conflicted = [f for f in git(ctx, repo, "diff", "--name-only", "--diff-filter=U").stdout.split() if f]
            if not conflicted:
                raise ActionError("git revert failed without a merge conflict")
            ctx.say(f"conflict in: {', '.join(conflicted)}  (Domain A and B edit adjacent lines)")
            patch = load_patch(ctx.settings.patches_dir, resolution)
            unresolved = [f for f in conflicted if f not in patch.files]
            if unresolved:
                raise ActionError(f"no resolution for {unresolved} in patch set {resolution}")
            apply_to_tree(patch, Path(repo))
            git(ctx, repo, "add", *conflicted)
            git(ctx, repo, "revert", "--continue")
        git(ctx, repo, "push", "--quiet", "origin", f"HEAD:refs/heads/{branch}")
        return git(ctx, repo, "log", "-1", "--format=%h %s").stdout.strip()


def promote_release(ctx: Ctx, *, branch: str, url: str | None = None) -> str:
    with tempfile.TemporaryDirectory() as tmp:
        repo = clone(ctx, tmp, remote_url(ctx, url), "main")
        git(ctx, repo, "fetch", "--quiet", "origin", branch)
        git(ctx, repo, "checkout", "-B", "main", "origin/main")
        base = git(ctx, repo, "merge-base", "main", f"origin/{branch}").stdout.strip()
        git(ctx, repo, "reset", "--hard", base)
        head_is_release = git(ctx, repo, "rev-parse", "HEAD").stdout.strip() == git(ctx, repo, "rev-parse", f"origin/{branch}").stdout.strip()
        # If HEAD already equals the release tip an empty merge commit keeps `tag_main` working (YYYY-Qx.0)
        message = f"Merge branch '{branch}' into 'main'"
        if head_is_release:
            git(ctx, repo, "commit", "--allow-empty", "-m", message)
        else:
            git(ctx, repo, "merge", "--no-ff", f"origin/{branch}", "-m", message)
        git(ctx, repo, "push", "--quiet", "--force", "origin", "main")
        return git(ctx, repo, "log", "-1", "--format=%h %s").stdout.strip()


def reset_branches(ctx: Ctx, *, url: str | None = None, gl: GitLab | None = None) -> str:
    """dev -> v1.0.0, main -> 2026_DROP1.0; delete all other branches/tags. Protected branches are
    unprotected, force-pushed and protected again with the bootstrap rules."""
    gl = gl or GitLab(ctx)
    for name in ("main", "dev"):
        gl.req("DELETE", gl.p(f"/protected_branches/{quote(name)}"), ok=(200, 204, 404))
    try:
        with tempfile.TemporaryDirectory() as tmp:
            repo = clone(ctx, tmp, remote_url(ctx, url), "main")
            # baseline tags are annotated (bootstrap uses `git tag -a`): peel to the commit
            # ci.skip: pushing dev back to the baseline must not run tag_dev (it would tag the baseline commit v1.1.0)
            git(ctx, repo, "push", "--quiet", "--force", "-o", "ci.skip", "origin",
                "v1.0.0^{commit}:refs/heads/dev", "2026_DROP1.0^{commit}:refs/heads/main")
    finally:   # never leave dev/main unprotected, even when the push fails
        for name, rules in PROTECTED.items():
            gl.req("POST", gl.p("/protected_branches"), ok=(200, 201, 409), json={"name": name, **rules})
    for b in gl.req("GET", gl.p("/repository/branches"), params={"per_page": 100}).json():
        if b["name"] not in ("main", "dev", "changelog"):
            gl.delete_branch(b["name"])
    for t in gl.req("GET", gl.p("/repository/tags"), params={"per_page": 100}).json():
        if t["name"] not in ("v1.0.0", "2026_DROP1.0"):
            gl.req("DELETE", gl.p(f"/repository/tags/{quote(t['name'])}"), ok=(200, 204, 404))
    return "dev=v1.0.0, main=2026_DROP1.0, other branches/tags removed"


@register
class GitScript(Action):
    name = "git_script"
    SCRIPTS = ("revert_on_release", "promote_release", "reset_branches")

    def render(self, p: dict[str, Any], ctx: Ctx):
        require(p, "script")
        s = p["script"]
        if s == "revert_on_release":
            require(p, "branch", "tag")
            return [f"git checkout {p['branch']} && git revert --no-edit [-m 1 if the tag is a merge commit] {p['tag']}   # conflict -> resolve with patch set "
                    f"'{p.get('resolution', 'resolve-release-domain-a-only')}' -> git revert --continue && git push origin {p['branch']}"]
        if s == "promote_release":
            require(p, "branch")
            return [f"git checkout -B main origin/main && git reset --hard $(git merge-base main {p['branch']}) && "
                    f"git merge --no-ff {p['branch']} -m \"Merge branch '{p['branch']}' into 'main'\" && git push --force origin main"]
        if s == "reset_branches":
            return ["git push --force origin v1.0.0:dev 2026_DROP1.0:main   # + delete other branches/tags"]
        raise ActionError(f"unknown git script {s!r}; known: {self.SCRIPTS}")

    def run(self, p: dict[str, Any], ctx: Ctx):
        s = p["script"]
        if s == "revert_on_release":
            out = revert_on_release(ctx, branch=p["branch"], tag=p["tag"], url=p.get("url"),
                                    resolution=p.get("resolution", "resolve-release-domain-a-only"))
        elif s == "promote_release":
            out = promote_release(ctx, branch=p["branch"], url=p.get("url"))
        else:
            out = reset_branches(ctx, url=p.get("url"))
        return Result(output=out)
