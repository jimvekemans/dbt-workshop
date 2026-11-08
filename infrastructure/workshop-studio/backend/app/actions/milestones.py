"""SingleStore milestones: named restore points of the whole warehouse (manager/milestones.sh, mounted into the
studio at /app/bin/milestones.sh). The Developer Edition has no CREATE MILESTONE / BACKUP, so a milestone is a copy of
the stopped data volume: create/restore stop SingleStore for a few seconds. bootstrap_stack.py creates `baseline`;
the Reset action restores it, which also puts every model table, the run log, users and grants back."""
from __future__ import annotations

import shlex

from .base import Action, ActionError, Ctx, Result, register, require

OPS = ("list", "create", "restore", "delete")
BASELINE = "baseline"


def _argv(ctx: Ctx, op: str, name: str | None) -> list[str]:
    return ["bash", str(ctx.settings.milestones_script), op, *([name] if name else [])]


def milestone_exists(ctx: Ctx, name: str) -> bool:
    proc = ctx.sh(_argv(ctx, "list", None), check=False, timeout=120)
    return proc.returncode == 0 and any(line.split()[:1] == [name] for line in proc.stdout.splitlines())


@register
class Milestone(Action):
    """{action: milestone, op: create|restore|delete|list, name: baseline}"""
    name = "milestone"

    def _params(self, p):
        op = p.get("op", "list")
        if op not in OPS:
            raise ActionError(f"milestone op must be one of {OPS}, got {op!r}")
        name = p.get("name")
        if op != "list":
            require(p, "name")
        return op, name

    def render(self, p, ctx):
        op, name = self._params(p)
        line = "manager/milestones.sh " + " ".join(shlex.quote(a) for a in [op, *([name] if name else [])])
        if op in ("create", "restore"):
            line += "   # stops SingleStore for a few seconds"
        return [line]

    def run(self, p, ctx):
        op, name = self._params(p)
        if not ctx.settings.milestones_script.exists():
            raise ActionError(f"{ctx.settings.milestones_script} not found: is manager/milestones.sh mounted "
                              "(docker-compose.yml, MILESTONES_SCRIPT)?")
        proc = ctx.sh(_argv(ctx, op, name), check=False, timeout=900)
        if proc.returncode != 0:
            tail = (proc.stderr or proc.stdout).strip().splitlines()[-1:] or ["no output"]
            raise ActionError(f"milestone {op} failed: {tail[0]}")
        return Result(output=proc.stdout.strip().splitlines()[-1] if proc.stdout.strip() else f"milestone {op} done",
                      data={"stdout": proc.stdout})
