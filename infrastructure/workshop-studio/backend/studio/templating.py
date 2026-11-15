"""The small `{{ ... }}` language of scenario files.

Only six forms exist: `env`, `db('landing')`, `inputs.<id>`, `day(n)`, `now()` and (in outcome sentences) `value`. A tiny pattern matcher evaluates
them, so a scenario file can never run arbitrary code, and a misspelt name is a load-time error.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta, timezone
from typing import Any, Callable

from .models import Input

DATABASES = ("landing", "edw_dv_rdv", "edw_dv_bdv", "im_dimensional", "vdp_mgmt")
_BRACES = re.compile(r"\{\{(.*?)\}\}", re.S)
_WHOLE = re.compile(r"\{\{((?:(?!\}\}).)*)\}\}", re.S)
_INPUT = re.compile(r"inputs\.([a-z][a-z0-9_]*)")
_DAY = re.compile(r"day\(\s*(-?\d+)\s*\)")
_DB = re.compile(r"db\(\s*(?:'([a-z_]+)'|\"([a-z_]+)\")\s*\)")


class TemplateError(ValueError):
    """A scenario file uses a name the language does not know."""


@dataclass
class TemplateContext:
    env: str
    inputs: dict[str, Any] = field(default_factory=dict)
    today: Callable[[], date] = date.today                 # the real current date
    clock: Callable[[], datetime] = lambda: datetime.now(timezone.utc)
    value: Any = None                                      # the number a step's outcome check returned

    def with_env(self, env: str) -> "TemplateContext":
        return TemplateContext(env, self.inputs, self.today, self.clock, self.value)

    def with_inputs(self, inputs: dict[str, Any]) -> "TemplateContext":
        return TemplateContext(self.env, inputs, self.today, self.clock, self.value)

    def with_value(self, value: Any) -> "TemplateContext":
        return TemplateContext(self.env, self.inputs, self.today, self.clock, value)


def _evaluate(expression: str, ctx: TemplateContext) -> Any:
    expression = expression.strip()
    if expression == "env":
        return ctx.env
    if expression == "value":
        if ctx.value is None:
            raise TemplateError("'{{ value }}' is only available in the sentences of an outcome check")
        number = ctx.value
        if isinstance(number, (int, float)) and not isinstance(number, bool) and number == int(number):
            return f"{int(number):,}"   # a whole number reads as 3,432
        return number
    if expression == "now()":
        return ctx.clock().isoformat(timespec="seconds")
    if m := _INPUT.fullmatch(expression):
        if m[1] not in ctx.inputs:
            raise TemplateError(f"unknown input '{m[1]}'")
        return ctx.inputs[m[1]]
    if m := _DAY.fullmatch(expression):
        return (ctx.today() + timedelta(days=int(m[1]))).isoformat()
    if m := _DB.fullmatch(expression):
        name = m[1] or m[2]
        if name not in DATABASES:
            raise TemplateError(f"unknown database '{name}' (known: {', '.join(DATABASES)})")
        return f"{ctx.env}_{name}"
    raise TemplateError(f"unknown expression '{{{{ {expression} }}}}'")


def render(value: Any, ctx: TemplateContext) -> Any:
    """Render every string in a nested value. A string that is exactly one expression keeps the value's own type
    (so `{{ inputs.count }}` stays a number); otherwise expressions are spliced in as text."""
    if isinstance(value, str):
        whole = _WHOLE.fullmatch(value.strip())
        if whole:
            return _evaluate(whole[1], ctx)
        return _BRACES.sub(lambda m: str(_evaluate(m[1], ctx)), value)
    if isinstance(value, list):
        return [render(v, ctx) for v in value]
    if isinstance(value, dict):
        return {k: render(v, ctx) for k, v in value.items()}
    return value


def render_item(item: dict[str, Any], ctx: TemplateContext) -> tuple[dict[str, Any], str]:
    """Render an action or evidence item. Returns it and the environment it runs in: its own `env:` when given,
    else the scenario's. `db(...)` inside the item uses that environment."""
    env = render(item["env"], ctx) if item.get("env") else ctx.env
    return render(item, ctx.with_env(env)), env


def render_input(inp: Input, ctx: TemplateContext) -> Input:
    """The input with its default and day/number bounds rendered, ready for `Input.coerce`."""
    bounds = {k: render(getattr(inp, k), ctx) for k in ("default", "min", "max")}
    if inp.mark:
        bounds["mark"] = inp.mark.model_copy(update={"day": render(inp.mark.day, ctx)})
    return inp.model_copy(update=bounds)
