"""Shared contract between the engine, the actions, the cleaners and the evidence providers.

An *action* is one thing a demo does to the stack (a `do:` / `setup:` item of a scenario).
A *cleaner* undoes one kind of touched resource.
An *evidence provider* reads what the audience looks at (a `show:` item).

Each registers itself with a decorator and declares its parameters as a pydantic model, so scenario files are
validated at load time against exactly what the code accepts.
"""
from __future__ import annotations

import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable

from pydantic import BaseModel, ConfigDict


class Params(BaseModel):
    """Base of every parameter model: unknown keys are errors, so a typo in a scenario file fails at load time."""

    model_config = ConfigDict(extra="forbid")
    env: str | None = None  # filled with the scenario's environment when omitted


class Cancelled(Exception):
    """The presenter pressed Cancel."""


class StepFailed(Exception):
    """An action did not get the outcome the scenario expects. `summary` is one plain sentence for the audience."""

    def __init__(self, summary: str, details: str = "", touched: set[str] | None = None):
        super().__init__(summary)
        self.summary, self.details = summary, details
        # what the action changed before it failed: still has to be undone by the next clean-up
        self.touched: set[str] = set(touched or ())


@dataclass
class Result:
    summary: str                      # one plain sentence: what happened
    details: str = ""                 # technical text for the "Technical details" disclosure
    touched: set[str] = field(default_factory=set)   # resources to undo later, e.g. "variable:dev:dbt_env_vars"
    data: dict[str, Any] = field(default_factory=dict)  # for the `result` evidence item


@dataclass
class Ctx:
    """What an action, cleaner or provider gets. `settings` is studio.config.Settings, `feeder` is studio.feeder.Feeder."""

    env: str
    settings: Any
    feeder: Any
    cancel: threading.Event = field(default_factory=threading.Event)
    progress: Callable[[str], None] = lambda text: None   # a plain sentence shown while the action runs
    last_result: Result | None = None                      # result of the previous action of the same step
    results: dict[str, Result] = field(default_factory=dict)   # newest result per action name in this step

    def sleep(self, seconds: float) -> None:
        """Sleep that ends early (raising Cancelled) when the presenter cancels."""
        if self.cancel.wait(seconds):
            raise Cancelled()

    def check_cancel(self) -> None:
        if self.cancel.is_set():
            raise Cancelled()

    def wait_until(self, probe: Callable[[], Any], timeout: float, every: float = 3.0, waiting: str = "") -> Any:
        """Poll `probe` until it returns something truthy; StepFailed after `timeout` seconds."""
        deadline = time.monotonic() + timeout
        while True:
            value = probe()
            if value:
                return value
            if time.monotonic() >= deadline:
                raise StepFailed(f"Gave up after {int(timeout)} seconds{': ' + waiting if waiting else ''}.")
            self.sleep(every)


@dataclass
class Spec:
    name: str
    params: type[Params]
    run: Callable[..., Any]
    label: Callable[[Any], str] | None = None   # plain progress label for the UI, from the validated params


ACTIONS: dict[str, Spec] = {}     # "airflow.run_dag" -> Spec; run(ctx, params) -> Result
CLEANERS: dict[str, Callable[[Ctx, str], Result]] = {}   # touched-resource prefix ("variable", "landing", ...) -> fn
EVIDENCE: dict[str, Spec] = {}    # "table" -> Spec; run(ctx, params) -> JSON-serialisable dict for the UI


def action(name: str, params: type[Params], label: Callable[[Any], str] | None = None):
    def register(fn):
        ACTIONS[name] = Spec(name, params, fn, label)
        return fn
    return register


def cleaner(prefix: str):
    """Registers the undo of every touched resource `prefix:...`. Cleaners must be idempotent."""
    def register(fn):
        CLEANERS[prefix] = fn
        return fn
    return register


def evidence(name: str, params: type[Params]):
    def register(fn):
        EVIDENCE[name] = Spec(name, params, fn)
        return fn
    return register


# Order in which touched resources are undone: cheap and independent first, rebuilds last.
CLEAN_ORDER = ("variable", "feed", "sql", "mail", "git", "version", "developer", "ci", "landing", "models")
