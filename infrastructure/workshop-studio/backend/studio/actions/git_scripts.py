"""Git scripts that need a real clone (revert on a release branch, promote a release) and the cleaner `git`.

The commands are the ones of the dbt-release-flow skill, run with the `git` CLI in a temporary clone. The cleaner
puts GitLab back to the recorded pristine state (see `gitlab.snapshot`): closes demo proposals, deletes demo and
release branches and every newer tag, and puts `dev` and `main` back on their baseline commits when they moved.
"""

from __future__ import annotations

import re
import tempfile
from pathlib import Path
from typing import Any

from pydantic import Field

from ..patches import apply_to_tree, load_patch
from . import clients
from .base import Ctx, Params, Result, StepFailed, action, cleaner
from .clients import GitLabApi, quote
from .gitlab import KEEP_ALIVE, connect

GIT_ENV = {
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_EDITOR": "true",
    "GIT_AUTHOR_NAME": "Workshop Studio",
    "GIT_AUTHOR_EMAIL": "studio@workshop.local",
    "GIT_COMMITTER_NAME": "Workshop Studio",
    "GIT_COMMITTER_EMAIL": "studio@workshop.local",
}
PROTECTION = {  # the rules the stack's bootstrap gives the two long-lived branches
    "main": {"push_access_level": 40, "merge_access_level": 40, "allow_force_push": True},
    "dev": {"push_access_level": 40, "merge_access_level": 30, "allow_force_push": False},
}


class RevertParams(Params):
    branch: str
    tag: str
    resolution: str | None = Field(
        None, description="patch dir that resolves the conflict the revert is known to cause"
    )


class PromoteParams(Params):
    branch: str


def remote_url(settings: Any) -> str:
    scheme, rest = settings.gitlab_url.split("://", 1)
    return f"{scheme}://root:{settings.gitlab_pat}@{rest}/{clients.GITLAB_PROJECT}.git"


def git(ctx: Ctx, repo: str, *args: str, check: bool = True) -> clients.Proc:
    proc = clients.run(ctx, ["git", *args], cwd=repo, env=GIT_ENV, timeout=180)
    if check and proc.returncode != 0:
        raise StepFailed(
            "The code server did not accept a step.", f"git {' '.join(args[:3])}: {proc.text[-500:]}"
        )
    return proc


def clone(ctx: Ctx, tmp: str, branch: str) -> str:
    repo = str(Path(tmp) / "repo")
    proc = clients.run(
        ctx, ["git", "clone", "--quiet", "-b", branch, remote_url(ctx.settings), repo], env=GIT_ENV, timeout=180
    )
    if proc.returncode != 0:
        raise StepFailed("The code could not be downloaded.", proc.text[-500:])
    git(ctx, repo, "fetch", "--quiet", "--tags", "origin")
    return repo


def revert(ctx: Ctx, repo: str, tag: str, resolution: str | None) -> str:
    """`git revert <tag>`; a merge commit needs `-m 1`; the known conflict is resolved with the `resolution` patch."""
    parents = git(ctx, repo, "rev-list", "--parents", "-n", "1", tag).stdout.split()[1:]
    proc = git(ctx, repo, "revert", "--no-edit", *(["-m", "1"] if len(parents) > 1 else []), tag, check=False)
    if proc.returncode == 0:
        return "revert applied cleanly"
    conflicted = git(ctx, repo, "diff", "--name-only", "--diff-filter=U").stdout.split()
    if not conflicted or resolution is None:
        raise StepFailed("Taking the change out of the copy for the release did not work.", proc.text[-500:])
    patch = load_patch(ctx.settings.patches_dir, resolution)
    if unresolved := [f for f in conflicted if f not in patch.files]:
        raise StepFailed(
            "Taking the change out needs a manual decision.", f"no resolution for {unresolved} in {resolution}"
        )
    apply_to_tree(patch, Path(repo))
    git(ctx, repo, "add", *conflicted)
    git(ctx, repo, "revert", "--continue")
    return f"revert conflicted in {', '.join(conflicted)}; resolved with {resolution}"


VERSION_TAG = re.compile(r"^v\d+\.\d+\.\d+$")
MERGED_BRANCH = re.compile(r"Merge branch '([^']+)'")


def change_words(title: str, branch: str, version: str | None) -> str:
    """"the reviews team's change (v1.2.0), review scores overview"; a branch that is not a team's gives "review
    scores overview (v1.2.0)"."""
    team = branch.rsplit("/", 1)[-1]
    if team.startswith("team-") and team[5:]:
        return f"the {team[5:].replace('-', ' ')} team's change" + (f" ({version})" if version else "") + f", {title}"
    return title + (f" ({version})" if version else "")


def release_contents(ctx: Ctx, repo: str) -> tuple[list[str], list[str]]:
    """The accepted changes on the release branch since `main`, as (taken out, still in), each in plain words with the
    team and the number of the change: "the reviews team's change (v1.2.0), review scores overview". A change is one merge
    commit; the title is the first line of its message body (the proposal's title), the team comes from the proposal's
    branch (`team-<name>`) and the number from the version tag on the merge."""
    log = git(ctx, repo, "log", "--first-parent", "--format=%H%x1f%s%x1f%b%x1e", "origin/main..HEAD", check=False)
    entries = [e.strip("\n").split("\x1f") for e in log.stdout.split("\x1e") if e.strip()]
    reverted = {
        sha for e in entries if e[1].startswith("Revert") for sha in re.findall(r"reverts commit ([0-9a-f]{40})", e[2])
    }
    out, kept = [], []
    for sha, subject, body in (e for e in entries if len(e) == 3 and e[1].startswith("Merge branch")):
        title = next((line for line in body.splitlines() if line.strip()), subject)
        title = re.sub(r"^(feat|fix|chore)(\([^)]*\))?:\s*", "", title.strip())
        tags = git(ctx, repo, "tag", "--points-at", sha, check=False).stdout.split()
        version = next((t for t in tags if VERSION_TAG.match(t)), None)
        branch = MERGED_BRANCH.match(subject)
        (out if sha in reverted else kept).append(change_words(title, branch[1] if branch else "", version))
    return out, kept


@action("git.revert_on_release", RevertParams, lambda p: "Taking one team's change out of the copy for the release")
def revert_on_release(ctx: Ctx, p: RevertParams) -> Result:
    connect(ctx)
    with tempfile.TemporaryDirectory() as tmp:
        repo = clone(ctx, tmp, p.branch)
        how = revert(ctx, repo, p.tag, p.resolution)
        git(ctx, repo, "push", "--quiet", "origin", f"HEAD:refs/heads/{p.branch}")
        head = git(ctx, repo, "log", "-1", "--format=%h %s").stdout.strip()
        out, kept = release_contents(ctx, repo)
    reasons = [f"Taken out: {t}" for t in out] + [f"Still in: {t}" for t in kept]
    return Result(
        "The change was taken out of the copy for the release; everything else is still in.",
        details=f"{how}\n{head}\nreverted tag {p.tag} on {p.branch}",
        touched={"git"},
        data={"reasons": reasons, "taken_out": out, "still_in": kept},
    )


def promote(ctx: Ctx, repo: str, branch: str) -> str:
    """merge-base reset + `--no-ff` merge + force push: what the job promote_release_to_main does."""
    git(ctx, repo, "fetch", "--quiet", "origin", branch)
    git(ctx, repo, "checkout", "-B", "main", "origin/main")
    base = git(ctx, repo, "merge-base", "main", f"origin/{branch}").stdout.strip()
    git(ctx, repo, "reset", "--hard", base)
    message = f"Merge branch '{branch}' into 'main'"
    if git(ctx, repo, "rev-parse", "HEAD").stdout == git(ctx, repo, "rev-parse", f"origin/{branch}").stdout:
        git(ctx, repo, "commit", "--allow-empty", "-m", message)  # an empty merge keeps the release tagging job working
    else:
        git(ctx, repo, "merge", "--no-ff", f"origin/{branch}", "-m", message)
    git(ctx, repo, "push", "--quiet", "--force", "origin", "main")
    return git(ctx, repo, "log", "-1", "--format=%h %s").stdout.strip()


@action("git.promote_release", PromoteParams, lambda p: "Letting the release go live in validation")
def promote_release(ctx: Ctx, p: PromoteParams) -> Result:
    connect(ctx)
    with tempfile.TemporaryDirectory() as tmp:
        head = promote(ctx, clone(ctx, tmp, "main"), p.branch)
    return Result(
        "The release goes live in validation; production waits for a person.", details=head, touched={"git", "version:val"}
    )


# --- cleaner --------------------------------------------------------------------------------------


def close_demo_proposals(api: GitLabApi) -> int:
    opened = [mr for mr in api.get("/merge_requests", state="opened") if mr["source_branch"].startswith("demo/")]
    for mr in opened:
        api.req("PUT", api.p(f"/merge_requests/{mr['iid']}"), json={"state_event": "close"})
    return len(opened)


def move_back(ctx: Ctx, api: GitLabApi, moved: dict[str, str]) -> None:
    """Force `dev` / `main` back on their baseline commits: unprotect, push without CI, protect again."""
    for name in moved:
        api.req("DELETE", api.p(f"/protected_branches/{quote(name)}"), ok=(200, 204, 404))
    try:
        with tempfile.TemporaryDirectory() as tmp:
            repo = clone(ctx, tmp, "main")
            git(ctx, repo, "fetch", "--quiet", "origin", *[f"{KEEP_ALIVE}{n}" for n in moved])
            # ci.skip: moving dev back must not run tag_dev, which would number the baseline commit again
            git(
                ctx,
                repo,
                "push",
                "--quiet",
                "--force",
                "-o",
                "ci.skip",
                "origin",
                *[f"{sha}:refs/heads/{name}" for name, sha in moved.items()],
            )
    finally:  # never leave dev or main unprotected, even when the push failed
        for name in moved:
            api.req("POST", api.p("/protected_branches"), ok=(200, 201, 409), json={"name": name, **PROTECTION[name]})


@cleaner("git")
def clean_git(ctx: Ctx, resource: str) -> Result:
    api, base = connect(ctx)
    closed = close_demo_proposals(api)
    for name in api.branch_names():
        if name not in base["branches"] and name.startswith(("demo/", "release/")):
            api.delete_branch(name)
    for tag in set(api.tag_names()) - set(base["tags"]):
        api.req("DELETE", api.p(f"/repository/tags/{quote(tag)}"), ok=(200, 204, 404))
        # the packaged version of a number that no longer exists would be picked as "the newest" by a later demo
        clients.docker(ctx, "rmi", f"dbt-vdp:{tag}", check=False)
    moved = {n: base[n] for n in ("dev", "main") if api.branch_sha(n) != base[n]}
    if moved:
        move_back(ctx, api, moved)
    return Result(
        "The code is back to how it was before the demos.",
        details=f"closed {closed} proposals; moved back: {', '.join(moved) or 'nothing'}",
    )
