"""Deterministic generator of one calendar day of a small web shop.

Pure: the same (day, conditions, seed) always gives the same rows and ids, so re-seeding restores the baseline.
Everything that "happens" on `day` is returned: customers placing orders, payments, status changes of earlier orders
(shipped, delivered), parcel scans, reviews that arrive a few days after delivery, and web visits.

Each order has a fixed life plan derived from (seed, order day, index): its payments, the carrier scans, the delivery
and the review. A day collects the events of the plans of the last LOOKBACK days that fall on that day.
`paused` is not handled here: it is delivery behaviour (see Feeder). `late_days` shifts a feed's arrival times, and
`arrivals` picks the rows whose arrival time lies in a time span (the Feeder's only way to get rows).
"""
from __future__ import annotations

import hashlib
import random
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta
from functools import lru_cache
from typing import Any

from .conditions import FEED_TABLES, FEEDS, Conditions, FeedCondition

ORDERS_PER_DAY = 40
WEEKDAY_FACTOR = (1.05, 1.0, 1.0, 0.98, 1.12, 0.75, 0.68)   # Monday .. Sunday
HOUR_WEIGHTS = (2, 3, 5, 7, 8, 8, 7, 6, 6, 7, 8, 9, 8, 6, 3)  # purchase hours 07 .. 21
LOOKBACK = 12            # days an order stays "alive": pick-up, transit, delivery (<= 7) and review (<= +4)
PERSONS = 2500
CITIES = (
    ("sao paulo", "SP", 1310), ("rio de janeiro", "RJ", 20040), ("belo horizonte", "MG", 30110),
    ("curitiba", "PR", 80010), ("porto alegre", "RS", 90010), ("salvador", "BA", 40020),
    ("brasilia", "DF", 70040), ("recife", "PE", 50010), ("fortaleza", "CE", 60010), ("campinas", "SP", 13010),
)
CATEGORIES = ("garden", "toys", "books", "kitchen", "sport", "beauty", "electronics", "furniture")
REFERRERS = ("google", "direct", "newsletter", "instagram", "comparison_site")
REFERRER_WEIGHTS = (40, 25, 15, 12, 8)
COMMENTS = (
    ("Great", "Arrived earlier than promised, well packed."), ("Good value", "Exactly as described."),
    ("Slow", "The parcel took longer than I hoped."), ("Not happy", "The product was not what I expected."),
    ("Perfect", "Will order again."), ("Late", "Delivery was after the promised date."),
)
STATUS_V2 = {"picked_up": "PU", "in_transit": "IT", "delivered": "DL"}


def _hex(*parts: Any) -> str:
    return hashlib.md5("|".join(str(p) for p in parts).encode()).hexdigest()


def _at(day: date, hour: int, minute: int, second: int) -> datetime:
    return datetime.combine(day, time(hour, minute, second))


@dataclass(frozen=True)
class Plan:
    order_id: str
    customer_id: str
    person: int
    purchase: datetime
    approved: datetime
    estimated: datetime
    payments: tuple[tuple[int, str, int, float], ...]
    pickup: datetime
    transits: tuple[datetime, ...]
    delivered: datetime
    review: tuple[datetime, datetime, int, str | None, str | None] | None
    web: tuple[tuple[str, str, str, datetime, str | None], ...]   # (event_id, type, page, ts, customer_ref)
    visitor_id: str
    referrer: str
    jitter: int                                                    # seconds of ingestion delay seed


@lru_cache(maxsize=2048)
def _plans(seed: int, day: date) -> tuple[Plan, ...]:
    rng = random.Random(f"{seed}|count|{day.isoformat()}")
    n = max(8, round(ORDERS_PER_DAY * WEEKDAY_FACTOR[day.weekday()] + rng.gauss(0, 3)))
    return tuple(_plan(seed, day, i) for i in range(n))


def _plan(seed: int, day: date, i: int) -> Plan:
    r = random.Random(f"{seed}|order|{day.isoformat()}|{i}")
    order_id, customer_id = _hex(seed, "order", day, i), _hex(seed, "customer", day, i)
    purchase = _at(day, r.choices(range(7, 22), HOUR_WEIGHTS)[0], r.randrange(60), r.randrange(60))
    approved = purchase + timedelta(minutes=r.randint(8, 55))
    person = r.randrange(300) if r.random() < 0.3 else r.randrange(PERSONS)  # a few frequent buyers

    total = round(min(900.0, max(12.0, r.lognormvariate(4.4, 0.7))), 2)
    kinds = r.choices(("credit_card", "boleto", "voucher", "debit_card"), (74, 20, 5, 1), k=2)
    if r.random() < 0.18:
        first = round(total * r.uniform(0.3, 0.7), 2)
        amounts = (first, round(total - first, 2))
    else:
        amounts = (total,)
    payments = tuple((s + 1, kinds[s], r.randint(1, 10) if kinds[s] == "credit_card" else 1, a)
                     for s, a in enumerate(amounts))

    pickup_day = day + timedelta(days=r.choice((1, 1, 1, 2)))
    pickup = _at(pickup_day, r.randint(8, 12), r.randrange(60), r.randrange(60))
    deliver_day = pickup_day + timedelta(days=r.choice((1, 2, 2, 3, 3, 4, 5)))
    delivered = _at(deliver_day, r.randint(9, 19), r.randrange(60), r.randrange(60))
    span = (delivered - pickup - timedelta(hours=6)).total_seconds()
    transits = tuple(sorted(pickup + timedelta(hours=3, seconds=int(r.random() * span))
                            for _ in range(r.choice((0, 1, 1, 2)))))
    estimated = _at(day + timedelta(days=r.choices(range(5, 11), (1, 2, 3, 3, 2, 1))[0]), 0, 0, 0)

    review = None
    if r.random() < 0.6:
        late = delivered >= estimated
        score = r.choices((1, 2, 3, 4, 5), (30, 10, 12, 18, 30) if late else (6, 3, 8, 22, 61))[0]
        created = _at(deliver_day + timedelta(days=r.randint(1, 4)), 0, 0, 0)
        answered = created + timedelta(hours=r.randint(10, 20), minutes=r.randrange(60))
        title, message = (r.choice(COMMENTS) if r.random() < 0.4 else (None, None))
        review = (created, answered, score, title, message)

    visitor_id = _hex(seed, "visitor", day, i)
    referrer = r.choices(REFERRERS, REFERRER_WEIGHTS)[0]
    n_events = r.randint(2, 6)
    events: list[tuple[str, str, str, datetime, str | None]] = []
    ts = purchase - timedelta(seconds=r.randint(20, 240))
    kinds_seq = ["checkout", "add_to_cart"] + ["page_view"] * (n_events - 2)   # built backwards in time
    for k, kind in enumerate(kinds_seq):
        page = {"checkout": "/checkout", "add_to_cart": "/cart"}.get(
            kind, r.choice(("/", f"/category/{r.choice(CATEGORIES)}", f"/product/{r.randint(100, 999)}")))
        events.append((_hex(seed, "web", day, i, k), kind, page, ts, customer_id if kind != "page_view" else None))
        ts -= timedelta(seconds=r.randint(40, 360))
    return Plan(order_id, customer_id, person, purchase, approved, estimated, payments, pickup, transits, delivered,
                review, tuple(reversed(events)), visitor_id, referrer, r.randrange(10_000))


def _person_row(seed: int, person: int) -> tuple[str, str, str, str]:
    unique_id = _hex(seed, "person", person)
    city, state, zip_base = CITIES[int(unique_id[:4], 16) % len(CITIES)]
    return unique_id, f"{zip_base + int(unique_id[4:8], 16) % 90:05d}", city, state


def _ingest(day: date, ts: datetime, minutes_lo: int, minutes_hi: int, jitter: int) -> datetime:
    """Arrival of a record created at `ts`: a few minutes later, never after the end of that day."""
    seconds = minutes_lo * 60 + jitter % max(1, (minutes_hi - minutes_lo) * 60)
    return min(ts + timedelta(seconds=seconds), _at(day, 23, 59, 0))


def _malformed(customer_id: str) -> str:
    kind = int(customer_id[:2], 16) % 3
    return customer_id[:31] if kind == 0 else customer_id[:31] + "g" if kind == 1 else "cust_" + customer_id[:27]


def _is_dirty(seed: int, order_id: str, pct: float) -> bool:
    return pct > 0 and int(_hex(seed, "dirty", order_id)[:8], 16) / 0xFFFFFFFF * 100 < pct


def _carrier_payload(plan: Plan, event: str, ts: datetime, v2: bool) -> dict[str, Any]:
    shipment = _hex("shipment", plan.order_id)[:20]
    if not v2:   # local business time
        return {"shipment_id": shipment, "order_ref": plan.order_id, "event": event,
                "event_time": ts.strftime("%Y-%m-%d %H:%M:%S")}
    return {"shipmentId": shipment, "orderReference": plan.order_id, "status": {"code": STATUS_V2[event]},
            "eventTime": (ts + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%SZ")}   # UTC, local is UTC-3


def generate_day(day: date, conditions: Conditions | None = None, seed: int = 20260101,
                 first_day: date | None = None) -> dict[str, list[dict[str, Any]]]:
    """Everything that happens on `day`, keyed by landing table name.

    `first_day` is the first day of the history: orders placed before it do not exist, so their later
    events (scans, status changes, reviews) are left out.
    """
    conditions = conditions or Conditions.default()
    out: dict[str, list[dict[str, Any]]] = {table: [] for table in FEED_TABLES.values()}
    orders_c, carrier_c, web_c = conditions.get("orders"), conditions.get("carrier"), conditions.get("web")
    customers, orders = out["raw_olist_customers"], out["raw_olist_orders"]
    payments, reviews = out["raw_olist_order_payments"], out["raw_olist_order_reviews"]
    scans, web = out["raw_carrier_tracking_events"], out["raw_web_events"]
    carrier_v2 = carrier_c.v2_on(day + timedelta(days=carrier_c.late_days))   # the format follows the arrival day

    for back in range(LOOKBACK, -1, -1):
        d0 = day - timedelta(days=back)
        if first_day and d0 < first_day:
            continue
        for plan in _plans(seed, d0):
            j = plan.jitter
            cust_id = plan.customer_id
            if _is_dirty(seed, plan.order_id, orders_c.dirty_pct):
                cust_id = _malformed(plan.customer_id)

            def order_row(status: str, at: datetime, carrier: datetime | None, delivered: datetime | None):
                return {"order_id": plan.order_id, "customer_id": cust_id, "order_status": status,
                        "order_purchase_timestamp": plan.purchase, "order_approved_at": plan.approved,
                        "order_delivered_carrier_date": carrier, "order_delivered_customer_date": delivered,
                        "order_estimated_delivery_date": plan.estimated,
                        "_ingested_at": _ingest(day, at, 2, 9, j)}

            if d0 == day:
                unique_id, zip_code, city, state = _person_row(seed, plan.person)
                customers.append({"customer_id": plan.customer_id, "customer_unique_id": unique_id,
                                  "customer_zip_code_prefix": zip_code, "customer_city": city,
                                  "customer_state": state, "_ingested_at": _ingest(day, plan.purchase, 0, 5, j)})
                orders.append(order_row("processing", plan.purchase, None, None))
                for seq, kind, installments, value in plan.payments:
                    payments.append({"order_id": plan.order_id, "payment_sequential": seq, "payment_type": kind,
                                     "payment_installments": installments, "payment_value": value,
                                     "_ingested_at": _ingest(day, plan.purchase, 1, 8, j + seq)})
                for event_id, kind, page, ts, ref in plan.web:
                    web.append({"event_id": event_id, "visitor_id": plan.visitor_id, "customer_ref": ref,
                                "event_type": kind, "page": page, "referrer": plan.referrer, "event_ts": ts,
                                "_ingested_at": _ingest(day, ts, 1, 5, j)})
            events = [("picked_up", plan.pickup)] + [("in_transit", t) for t in plan.transits] \
                + [("delivered", plan.delivered)]
            for n, (event, ts) in enumerate(events):
                if ts.date() != day:
                    continue
                at = _ingest(day, ts, 2, 9, j + n)
                scans.append({"event_id": _hex("scan", plan.order_id, event, n),
                              "payload": _carrier_payload(plan, event, ts, carrier_v2),
                              "_ingested_at": at})
                if event == "picked_up":
                    orders.append(order_row("shipped", ts, plan.pickup, None))
                elif event == "delivered":
                    orders.append(order_row("delivered", ts, plan.pickup, plan.delivered))
            if plan.review and plan.review[0].date() == day:
                created, answered, score, title, message = plan.review
                reviews.append({"review_id": _hex(seed, "review", plan.order_id), "order_id": plan.order_id,
                                "review_score": score, "review_comment_title": title,
                                "review_comment_message": message, "review_creation_date": created,
                                "review_answer_timestamp": answered, "_ingested_at": _ingest(day, answered, 2, 9, j)})

    web.extend(_anonymous_visits(seed, day))
    orders.extend(_bad_order_rows(seed, day, orders_c.bad_rows))
    scans.extend(_bad_scan_rows(seed, day, carrier_c.bad_rows, carrier_v2))
    if web_c.bots:
        web.extend(_crawler(seed, day, web_c.bots))
    for feed in FEEDS:   # duplicates last, so that they repeat every kind of row the feed has today
        _duplicate(out[FEED_TABLES[feed]], conditions.get(feed), seed, feed, day)
    for feed in FEEDS:   # a late feed delivers everything this many days after it was made
        late = conditions.get(feed).late_days
        if late:
            out[FEED_TABLES[feed]] = [{**row, "_ingested_at": row["_ingested_at"] + timedelta(days=late)}
                                      for row in out[FEED_TABLES[feed]]]
    return out


def arrivals(conditions: Conditions, seed: int, first_day: date | None, start: datetime, end: datetime,
             feeds: tuple[str, ...] | list[str] = FEEDS) -> dict[str, list[dict[str, Any]]]:
    """The rows of `feeds` that arrive from `start` (included) up to `end` (excluded), under `conditions`.

    Deterministic: the same span, conditions and seed always give the same rows, and two spans that follow each other
    give together exactly the rows of the span that covers both. `paused` is not applied here.
    """
    tables = {FEED_TABLES[f] for f in feeds}
    out: dict[str, list[dict[str, Any]]] = {t: [] for t in tables}
    if end <= start:
        return out
    oldest = start.date() - timedelta(days=max((conditions.get(f).late_days for f in feeds), default=0))
    day = oldest
    while day <= end.date():
        for table, rows in generate_day(day, conditions, seed, first_day=first_day).items():
            if table in tables:
                out[table].extend(r for r in rows if start <= r["_ingested_at"] < end)
        day += timedelta(days=1)
    return out


def _anonymous_visits(seed: int, day: date) -> list[dict[str, Any]]:
    r = random.Random(f"{seed}|anon|{day.isoformat()}")
    rows = []
    for v in range(round(r.randint(90, 130) * WEEKDAY_FACTOR[day.weekday()])):
        visitor, referrer = _hex(seed, "anon", day, v), r.choices(REFERRERS, REFERRER_WEIGHTS)[0]
        ts = _at(day, r.choices(range(7, 22), HOUR_WEIGHTS)[0], r.randrange(60), r.randrange(60))
        kinds = ["page_view"] * r.randint(1, 6) + (["add_to_cart"] if r.random() < 0.15 else [])
        for k, kind in enumerate(kinds):
            page = "/cart" if kind == "add_to_cart" else r.choice(("/", f"/category/{r.choice(CATEGORIES)}",
                                                                 f"/product/{r.randint(100, 999)}"))
            rows.append({"event_id": _hex(seed, "anonevt", day, v, k), "visitor_id": visitor, "customer_ref": None,
                         "event_type": kind, "page": page, "referrer": referrer, "event_ts": ts,
                         "_ingested_at": min(ts + timedelta(minutes=2), _at(day, 23, 59, 0))})
            ts += timedelta(seconds=r.randint(30, 300))
    return rows


def _early(day: date, k: int) -> datetime:
    """Faulty rows arrive in the first minutes of the day, so that they exist whatever time it is now."""
    return _at(day, 0, 1, 0) + timedelta(minutes=(k * 2) % 1400, seconds=(k * 13) % 60)


def _bad_order_rows(seed: int, day: date, count: int) -> list[dict[str, Any]]:
    rows = []
    for k in range(count):
        ts = _early(day, k)
        rows.append({"order_id": None, "customer_id": _hex(seed, "badcustomer", day, k), "order_status": "processing",
                     "order_purchase_timestamp": ts, "order_approved_at": ts + timedelta(minutes=20),
                     "order_delivered_carrier_date": None, "order_delivered_customer_date": None,
                     "order_estimated_delivery_date": _at(day + timedelta(days=7), 0, 0, 0),
                     "_ingested_at": ts + timedelta(minutes=5)})
    return rows


def _bad_scan_rows(seed: int, day: date, count: int, v2: bool) -> list[dict[str, Any]]:
    """Scans that arrive without an event id (the not_null test on stg_carrier_events.event_id fails on them)."""
    rows = []
    for k in range(count):
        ts = _early(day, k)
        shipment = _hex(seed, "badscan", day, k)[:20]
        if v2:
            payload = {"shipmentId": shipment, "orderReference": None, "status": {"code": "IT"},
                       "eventTime": (ts + timedelta(hours=3)).strftime("%Y-%m-%dT%H:%M:%SZ")}
        else:
            payload = {"shipment_id": shipment, "order_ref": None, "event": "in_transit",
                       "event_time": ts.strftime("%Y-%m-%d %H:%M:%S")}
        rows.append({"event_id": None, "payload": payload, "_ingested_at": ts + timedelta(minutes=4)})
    return rows


def _crawler(seed: int, day: date, views: int) -> list[dict[str, Any]]:
    visitor, step = _hex(seed, "bot", day), max(1, 86_000 // views)
    rows = []
    for i in range(views):
        ts = _at(day, 0, 0, 0) + timedelta(seconds=min(86_000, i * step))
        rows.append({"event_id": _hex(seed, "botevt", day, i), "visitor_id": visitor, "customer_ref": None,
                     "event_type": "page_view", "page": f"/product/{100 + i % 900}", "referrer": "direct",
                     "event_ts": ts, "_ingested_at": min(ts + timedelta(minutes=1), _at(day, 23, 59, 0))})
    return rows


def _duplicate(rows: list[dict[str, Any]], cond: FeedCondition, seed: int, feed: str, day: date) -> None:
    if cond.duplicate_pct <= 0:
        return
    r = random.Random(f"{seed}|dup|{feed}|{day.isoformat()}")
    rows.extend([dict(row) for row in list(rows) if r.random() * 100 < cond.duplicate_pct])
