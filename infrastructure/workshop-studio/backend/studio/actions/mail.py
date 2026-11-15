"""`mail.clear` and the cleaner `mail`: empty Mailpit, the inbox where Airflow's failure mails land."""

from __future__ import annotations

import requests

from . import clients
from .base import Ctx, Params, Result, StepFailed, action, cleaner


def clear_inbox(ctx: Ctx) -> None:
    try:
        r = requests.delete(f"{ctx.settings.mailpit_url}/api/v1/messages", timeout=10)
    except requests.RequestException as exc:
        raise StepFailed("The mailbox is not reachable.", clients.mask(ctx.settings, str(exc))) from exc
    if r.status_code != 200:
        raise StepFailed(
            "The mailbox could not be emptied.", f"DELETE /api/v1/messages -> {r.status_code}: {r.text[:200]}"
        )


@action("mail.clear", Params, lambda p: "Emptying the mailbox")
def mail_clear(ctx: Ctx, p: Params) -> Result:
    clear_inbox(ctx)
    return Result("The mailbox is empty.", details="DELETE /api/v1/messages")


@cleaner("mail")
def clean_mail(ctx: Ctx, resource: str) -> Result:
    clear_inbox(ctx)
    return Result("The mailbox is empty.")
