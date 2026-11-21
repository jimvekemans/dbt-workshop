"""`wait`: let the presenter's story catch up with the stack (a pause the audience can see the reason for)."""

from __future__ import annotations

from pydantic import Field

from ..names import format_number
from .base import Ctx, Params, Result, action


class WaitParams(Params):
    seconds: float = Field(gt=0, le=600)
    because: str = "Het systeem een moment de tijd geven"


@action("wait", WaitParams, lambda p: p.because)
def wait(ctx: Ctx, p: WaitParams) -> Result:
    ctx.progress(p.because)
    ctx.sleep(p.seconds)
    if p.seconds <= 2:   # a pause only there to give the step a visible moment: say what happened, not the wait
        return Result(f"{p.because.rstrip('.')}.")
    return Result(f"{format_number(p.seconds)} seconden gewacht: {p.because.rstrip('.')}.")
