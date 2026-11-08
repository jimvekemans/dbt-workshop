"""Action registry. An action has a `render()` (the exact command/SQL/API call shown in the Code tab and used by
dry-run) and a `run()` (executes it against the stack)."""
from __future__ import annotations

import shlex
import subprocess
import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable

from ..config import Settings


class ActionError(RuntimeError):
    pass


class PreconditionFailed(ActionError):
    """A scenario's `requires:` check is not met (e.g. GitLab is not at the baseline): nothing was run."""


class Cancelled(ActionError):
    """The presenter pressed Cancel: the action stopped at its next wait point."""


@dataclass
class Result:
    ok: bool = True
    output: str = ""
    data: dict[str, Any] = field(default_factory=dict)


@dataclass
class Ctx:
    settings: Settings
    dry_run: bool = False
    log: Callable[[str], None] = lambda line: None
    # per-process memo (e.g. GitLab project id)
    cache: dict[str, Any] = field(default_factory=dict)
    # set by the engine's Cancel button; every polling loop sleeps through ctx.sleep() so it can stop early
    cancel: threading.Event | None = None

    def say(self, line: str) -> None:
        self.log(self.settings.mask(line))

    def check_cancel(self) -> None:
        if self.cancel is not None and self.cancel.is_set():
            raise Cancelled("cancelled by the presenter")

    def sleep(self, seconds: float) -> None:
        """time.sleep that wakes up immediately when Cancel is pressed."""
        if self.cancel is None:
            time.sleep(seconds)
            return
        if self.cancel.wait(seconds):
            raise Cancelled("cancelled by the presenter")

    def sh(self, argv: list[str], *, cwd: str | None = None, check: bool = True, env: dict[str, str] | None = None,
           timeout: int = 600) -> subprocess.CompletedProcess:
        """Run a command, streaming (masked) output lines to the UI log."""
        self.say("$ " + self.settings.mask(" ".join(shlex.quote(a) for a in argv)))
        full_env = None
        if env:
            import os
            full_env = {**os.environ, **env}
        proc = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, env=full_env, timeout=timeout)
        for line in (proc.stdout + proc.stderr).splitlines():
            if line.strip():
                self.say("  " + line)
        if check and proc.returncode != 0:
            raise ActionError(f"command failed ({proc.returncode}): {argv[0]} {argv[1] if len(argv) > 1 else ''}")
        return proc


class Action:
    name = ""

    def render(self, p: dict[str, Any], ctx: Ctx) -> list[str]:  # noqa: D401
        raise NotImplementedError

    def run(self, p: dict[str, Any], ctx: Ctx) -> Result:
        raise NotImplementedError


REGISTRY: dict[str, Action] = {}


def register(cls: type[Action]) -> type[Action]:
    inst = cls()
    if not inst.name:
        raise ValueError(f"{cls} has no name")
    REGISTRY[inst.name] = inst
    return cls


def get(name: str) -> Action:
    try:
        return REGISTRY[name]
    except KeyError:
        raise ActionError(f"unknown action {name!r}; known: {sorted(REGISTRY)}") from None


def render(name: str, params: dict[str, Any], ctx: Ctx) -> list[str]:
    return get(name).render(params, ctx)


def execute(name: str, params: dict[str, Any], ctx: Ctx) -> Result:
    action = get(name)
    lines = action.render(params, ctx)
    if ctx.dry_run:
        for line in lines:
            ctx.say("[dry-run] " + line)
        return Result(ok=True, output="\n".join(lines), data={"dry_run": True})
    for line in lines:
        ctx.say("▶ " + line.splitlines()[0])
    return action.run(params, ctx)


def require(p: dict[str, Any], *keys: str) -> None:
    missing = [k for k in keys if k not in p or p[k] is None]
    if missing:
        raise ActionError(f"missing parameter(s): {', '.join(missing)}")
