"""Fault conditions of the data feeds, per environment and feed.

A condition is a small, validated set of knobs that makes one feed misbehave in a way the audience can see:
held back, late, delivered twice, dirty, in a new format, or flooded by a crawler. Defaults mean "healthy".
"""
from __future__ import annotations

from dataclasses import dataclass, fields, asdict
from datetime import date
from typing import Any

FEEDS = ("customers", "orders", "payments", "reviews", "carrier", "web")

FEED_TABLES = {
    "customers": "raw_olist_customers",
    "orders": "raw_olist_orders",
    "payments": "raw_olist_order_payments",
    "reviews": "raw_olist_order_reviews",
    "carrier": "raw_carrier_tracking_events",
    "web": "raw_web_events",
}

# Keys every feed understands, and the ones that only make sense for one feed.
COMMON_KEYS = ("paused", "late_days", "duplicate_pct")
FEED_KEYS = {"orders": ("dirty_pct", "bad_rows"), "carrier": ("format", "format_from", "bad_rows"),
             "web": ("bots",)}


@dataclass(frozen=True)
class FeedCondition:
    paused: bool = False        # nothing of this feed arrives
    late_days: int = 0          # each day's rows arrive this many days later
    duplicate_pct: float = 0.0  # this share of the day's rows is delivered twice
    dirty_pct: float = 0.0      # orders: share of orders with a malformed customer id
    bad_rows: int = 0           # orders: rows without an order id per day; carrier: scans without an event id
    format: str = "v1"          # carrier: payload shape ("v2" without format_from means v2 for every day)
    format_from: str | None = None  # carrier: ISO day from which the payload is v2
    bots: int = 0               # web: page views of one crawler visitor per day

    def to_dict(self) -> dict[str, Any]:
        """Only the values that differ from healthy."""
        default = FeedCondition()
        return {k: v for k, v in asdict(self).items() if v != getattr(default, k)}

    def is_default(self) -> bool:
        return self == FeedCondition()

    def v2_on(self, day: date) -> bool:
        """Does the carrier send the v2 payload on this arrival day?"""
        if self.format_from:
            return day >= date.fromisoformat(self.format_from)
        return self.format == "v2"


def _number(key: str, value: Any, lo: float, hi: float, integer: bool) -> Any:
    if isinstance(value, bool) or not isinstance(value, (int, float, str)):
        raise ValueError(f"{key} must be a number between {lo:g} and {hi:g}.")
    try:
        number = float(value)
    except ValueError:
        raise ValueError(f"{key} must be a number between {lo:g} and {hi:g}.") from None
    if integer and number != int(number):
        raise ValueError(f"{key} must be a whole number.")
    if not lo <= number <= hi:
        raise ValueError(f"{key} must be between {lo:g} and {hi:g}.")
    return int(number) if integer else number


def validate_feed(feed: str, values: dict[str, Any]) -> dict[str, Any]:
    """Check and normalise condition values for one feed; ValueError with a plain sentence otherwise."""
    if feed not in FEEDS:
        raise ValueError(f"Unknown feed '{feed}'. Choose one of: {', '.join(FEEDS)}.")
    allowed = COMMON_KEYS + FEED_KEYS.get(feed, ())
    out: dict[str, Any] = {}
    for key, value in values.items():
        if key not in allowed:
            raise ValueError(f"The {feed} feed has no setting '{key}'. It has: {', '.join(allowed)}.")
        if key == "paused":
            if value not in (True, False, 0, 1):
                raise ValueError("paused must be true or false.")
            out[key] = bool(value)
        elif key == "late_days":
            out[key] = _number(key, value, 0, 30, True)
        elif key in ("duplicate_pct", "dirty_pct"):
            out[key] = _number(key, value, 0, 100, False)
        elif key == "bad_rows":
            out[key] = _number(key, value, 0, 1000, True)
        elif key == "bots":
            out[key] = _number(key, value, 0, 200_000, True)
        elif key == "format":
            if value not in ("v1", "v2"):
                raise ValueError("format must be v1 or v2.")
            out[key] = value
        elif key == "format_from":
            if value in (None, ""):
                out[key] = None
            else:
                try:
                    out[key] = (value if isinstance(value, date) else date.fromisoformat(str(value))).isoformat()
                except ValueError:
                    raise ValueError("format_from must be a day such as 2026-03-14.") from None
    return out


class Conditions:
    """The conditions of one environment: a FeedCondition per feed."""

    def __init__(self, per_feed: dict[str, FeedCondition] | None = None):
        self._feeds = {feed: FeedCondition() for feed in FEEDS}
        self._feeds.update(per_feed or {})

    @classmethod
    def default(cls) -> "Conditions":
        return cls()

    @classmethod
    def from_dict(cls, data: dict[str, dict[str, Any]] | None) -> "Conditions":
        known = {f.name for f in fields(FeedCondition)}
        return cls({feed: FeedCondition(**{k: v for k, v in vals.items() if k in known})
                    for feed, vals in (data or {}).items() if feed in FEEDS})

    def get(self, feed: str) -> FeedCondition:
        return self._feeds[feed]

    def with_values(self, feed: str, **values: Any) -> "Conditions":
        clean = validate_feed(feed, values)
        merged = dict(self._feeds)
        merged[feed] = FeedCondition(**{**asdict(self._feeds[feed]), **clean})
        return Conditions(merged)

    def reset(self, feed: str | None = None) -> "Conditions":
        if feed is None:
            return Conditions()
        if feed not in FEEDS:
            raise ValueError(f"Unknown feed '{feed}'.")
        return Conditions({**self._feeds, feed: FeedCondition()})

    def to_dict(self) -> dict[str, dict[str, Any]]:
        return {feed: c.to_dict() for feed, c in self._feeds.items() if not c.is_default()}
