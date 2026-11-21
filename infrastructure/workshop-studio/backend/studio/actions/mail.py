"""`mail.clear` and the cleaner `mail`: empty Mailpit, the inbox where Airflow's failure mails land."""

from __future__ import annotations

import requests

from . import clients
from .base import Ctx, Params, Result, StepFailed, action, cleaner


def clear_inbox(ctx: Ctx) -> None:
    try:
        r = requests.delete(f"{ctx.settings.mailpit_url}/api/v1/messages", timeout=10)
    except requests.RequestException as exc:
        raise StepFailed("De mailbox is niet bereikbaar.", clients.mask(ctx.settings, str(exc))) from exc
    if r.status_code != 200:
        raise StepFailed(
            "De mailbox kon niet leeggemaakt worden.", f"DELETE /api/v1/messages -> {r.status_code}: {r.text[:200]}"
        )


@action("mail.clear", Params, lambda p: "De mailbox leegmaken")
def mail_clear(ctx: Ctx, p: Params) -> Result:
    clear_inbox(ctx)
    return Result("De mailbox is leeg.", details="DELETE /api/v1/messages")


@cleaner("mail")
def clean_mail(ctx: Ctx, resource: str) -> Result:
    clear_inbox(ctx)
    return Result("De mailbox is leeg.")
