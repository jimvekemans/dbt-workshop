"""Actions that steer the data feeds: make a feed misbehave, or play the last days of data again under it."""
from __future__ import annotations

from typing import Any, Literal

from pydantic import model_validator

from ..feeder.conditions import FEEDS, validate_feed
from .clients import ENV_WORDS
from .base import Params, Result, action, cleaner

FeedName = Literal["customers", "orders", "payments", "reviews", "carrier", "web"]

FEED_WORDS = {
    "customers": "customer records", "orders": "orders", "payments": "payments", "reviews": "reviews",
    "carrier": "parcel scans", "web": "website visits",
}


def _list(words: list[str]) -> str:
    return words[0] if len(words) == 1 else ", ".join(words[:-1]) + " and " + words[-1]


def _describe(feed: str, key: str, value: Any) -> str:
    what = FEED_WORDS[feed]
    if key == "paused":
        return f"Delivery of {what} is paused: nothing new arrives." if value else f"Delivery of {what} is running again."
    if key == "late_days":
        return f"{what.capitalize()} now arrive {value} day{'s' if value != 1 else ''} late." if value \
            else f"{what.capitalize()} arrive on time again."
    if key == "duplicate_pct":
        return f"{value:g}% of the {what} are now delivered twice."
    if key == "dirty_pct":
        return f"{value:g}% of the orders now carry a malformed customer number."
    if key == "bad_rows":
        if feed == "carrier":
            return f"Every day {value} parcel scans now arrive without a scan number."
        return f"Every day {value} orders now arrive without an order number."
    if key == "format":
        return f"The carrier now sends parcel scans in the {'new' if value == 'v2' else 'old'} format."
    if key == "format_from":
        return f"The carrier sends parcel scans in the new format from {value}." if value \
            else "The carrier format no longer switches on a date."
    return f"A crawler now visits the website {value:,} times a day."


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


@action("feed.condition", ConditionParams, label=lambda p: f"Changing how {FEED_WORDS[p.feed]} are delivered")
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


@action("feed.deliver", DeliverParams, label=lambda p: f"Sending the last {p.days} day{'s' if p.days != 1 else ''} of data again")
def feed_deliver(ctx, params: DeliverParams) -> Result:
    env = params.env or ctx.env
    feeds = params.feeds
    what = _list([FEED_WORDS[f] for f in (feeds or FEEDS)])
    if feeds is None:
        what = "every data feed"
    ctx.check_cancel()
    ctx.progress(f"Sending the last {params.days} day{'s' if params.days != 1 else ''} of data again in {ENV_WORDS[env]}.")
    by_table = ctx.feeder.deliver(env, days=params.days, feeds=feeds)
    total = sum(by_table.values())
    period = "today" if params.days == 1 else f"the last {params.days} days"
    return Result(summary=f"Sent {period} of {what} again in {ENV_WORDS[env]} with the current settings "
                          f"({total:,} rows arrived).",
                  details=f"feed.deliver {env} days={params.days} feeds={feeds or 'all'} rows={by_table}",
                  touched={f"landing:{env}"}, data={"rows": by_table})


@cleaner("feed")
def clean_feed(ctx, resource: str) -> Result:
    _, env, feed = resource.split(":")
    ctx.feeder.reset_conditions(env, feed)
    return Result(summary=f"The {FEED_WORDS.get(feed, feed)} feed to {ENV_WORDS[env]} is healthy again.")


@cleaner("landing")
def clean_landing(ctx, resource: str) -> Result:
    _, env = resource.split(":")
    ctx.feeder.seed(env, progress=ctx.progress)
    return Result(summary=f"The data in {ENV_WORDS[env]} is back to its starting point.")
