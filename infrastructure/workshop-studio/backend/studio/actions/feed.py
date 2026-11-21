"""Actions that steer the data feeds: make a feed misbehave, or play the last days of data again under it."""
from __future__ import annotations

from typing import Any, Literal

from pydantic import model_validator

from ..feeder.conditions import FEEDS, validate_feed
from ..names import day_words, format_number
from .clients import ENV_WORDS
from .base import Params, Result, action, cleaner

FeedName = Literal["customers", "orders", "payments", "reviews", "carrier", "web"]

FEED_WORDS = {
    "customers": "klantgegevens", "orders": "bestellingen", "payments": "betalingen", "reviews": "reviews",
    "carrier": "pakketscans", "web": "websitebezoeken",
}


def _list(words: list[str]) -> str:
    return words[0] if len(words) == 1 else ", ".join(words[:-1]) + " en " + words[-1]


def _days(n: int) -> str:
    return "1 dag" if n == 1 else f"{n} dagen"


def _describe(feed: str, key: str, value: Any) -> str:
    what = FEED_WORDS[feed]
    if key == "paused":
        return f"De levering van {what} is gepauzeerd: er komt niets nieuws binnen." if value \
            else f"De levering van {what} loopt weer."
    if key == "late_days":
        return f"{what.capitalize()} komen nu {_days(value)} te laat binnen." if value \
            else f"{what.capitalize()} komen weer op tijd binnen."
    if key == "duplicate_pct":
        return f"{format_number(value)}% van de {what} wordt nu twee keer geleverd."
    if key == "dirty_pct":
        return f"{format_number(value)}% van de bestellingen heeft nu een ongeldig klantnummer."
    if key == "bad_rows":
        if feed == "carrier":
            return f"Elke dag komen er nu {format_number(value)} pakketscans binnen zonder scannummer."
        return f"Elke dag komen er nu {format_number(value)} bestellingen binnen zonder bestelnummer."
    if key == "format":
        return f"De vervoerder stuurt de pakketscans nu in het {'nieuwe' if value == 'v2' else 'oude'} formaat."
    if key == "format_from":
        return f"De vervoerder stuurt de pakketscans in het nieuwe formaat vanaf {day_words(value)}." if value \
            else "Het formaat van de vervoerder schakelt niet meer om op een datum."
    return f"Een crawler bezoekt de website nu {format_number(value)} keer per dag."


class ConditionParams(Params):
    feed: FeedName
    set: dict[str, Any]

    @model_validator(mode="after")
    def _check(self) -> "ConditionParams":
        # Templated values ("{{ day(-4) }}") are only known at run time and validated then.
        known = {k: v for k, v in self.set.items() if not (isinstance(v, str) and "{{" in v)}
        try:
            validate_feed(self.feed, known)
        except ValueError as exc:
            raise ValueError(str(exc)) from None
        if not self.set:
            raise ValueError("set must name at least one condition.")
        return self


@action("feed.condition", ConditionParams, label=lambda p: f"De levering van {FEED_WORDS[p.feed]} aanpassen")
def feed_condition(ctx, params: ConditionParams) -> Result:
    env = params.env or ctx.env
    clean = validate_feed(params.feed, params.set)
    ctx.feeder.set_condition(env, params.feed, **clean)
    sentences = [_describe(params.feed, k, v) for k, v in clean.items()]
    return Result(summary=" ".join(sentences), details=f"feed.condition {env} {params.feed} {clean}",
                  touched={f"feed:{env}:{params.feed}"})


class DeliverParams(Params):
    days: int = 1
    feeds: list[FeedName] | None = None

    @model_validator(mode="after")
    def _check(self) -> "DeliverParams":
        if not 1 <= self.days <= 60:
            raise ValueError("days must be between 1 and 60.")
        return self


@action("feed.deliver", DeliverParams, label=lambda p: f"De data van de laatste {_days(p.days)} opnieuw sturen" if p.days != 1
        else "De data van vandaag opnieuw sturen")
def feed_deliver(ctx, params: DeliverParams) -> Result:
    env = params.env or ctx.env
    feeds = params.feeds
    what = _list([FEED_WORDS[f] for f in (feeds or FEEDS)])
    if feeds is None:
        what = "alle feeds"
    ctx.check_cancel()
    ctx.progress(f"De data van {'vandaag' if params.days == 1 else 'de laatste ' + _days(params.days)} wordt opnieuw "
                 f"gestuurd naar {ENV_WORDS[env]}.")
    by_table = ctx.feeder.deliver(env, days=params.days, feeds=feeds)
    total = sum(by_table.values())
    period = "vandaag" if params.days == 1 else f"de laatste {_days(params.days)}"
    return Result(summary=f"{what.capitalize()} van {period} opnieuw verstuurd naar {ENV_WORDS[env]} met de huidige "
                          f"instellingen ({format_number(total)} rijen binnengekomen).",
                  details=f"feed.deliver {env} days={params.days} feeds={feeds or 'all'} rows={by_table}",
                  touched={f"landing:{env}"}, data={"rows": by_table})


@cleaner("feed")
def clean_feed(ctx, resource: str) -> Result:
    _, env, feed = resource.split(":")
    ctx.feeder.reset_conditions(env, feed)
    return Result(summary=f"De feed van {FEED_WORDS.get(feed, feed)} naar {ENV_WORDS[env]} is weer in orde.")


@cleaner("landing")
def clean_landing(ctx, resource: str) -> Result:
    _, env = resource.split(":")
    ctx.feeder.seed(env, progress=ctx.progress)
    return Result(summary=f"De data in {ENV_WORDS[env]} staat weer op het beginpunt.")
