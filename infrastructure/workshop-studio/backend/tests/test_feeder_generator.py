import json
import re
import sys
from collections import Counter
from datetime import date, datetime, timedelta
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from studio.feeder.conditions import Conditions, validate_feed  # noqa: E402
from studio.feeder.generator import arrivals, generate_day  # noqa: E402

SEED = 7
FIRST = date(2026, 1, 1)
HEX32 = re.compile(r"^[0-9a-f]{32}$")


@pytest.fixture(scope="module")
def history():
    """30 baseline days, merged per table."""
    rows = {}
    for n in range(30):
        for table, part in generate_day(FIRST + timedelta(days=n), Conditions.default(), SEED, first_day=FIRST).items():
            rows.setdefault(table, []).extend(part)
    return rows


def test_deterministic_and_seed_dependent():
    day = date(2026, 2, 3)
    assert generate_day(day, Conditions.default(), SEED) == generate_day(day, Conditions.default(), SEED)
    other = generate_day(day, Conditions.default(), SEED + 1)
    assert other["raw_olist_orders"][0]["order_id"] != generate_day(day, None, SEED)["raw_olist_orders"][0]["order_id"]


def test_volume_and_weekly_rhythm(history):
    per_day = Counter(r["order_purchase_timestamp"].date() for r in history["raw_olist_orders"]
                      if r["order_status"] == "processing")
    assert 15 <= min(per_day.values()) and max(per_day.values()) <= 60
    weekday = [v for d, v in per_day.items() if d.weekday() < 5]
    weekend = [v for d, v in per_day.items() if d.weekday() >= 5]
    assert sum(weekday) / len(weekday) > sum(weekend) / len(weekend)


def test_ids_and_not_null(history):
    assert all(HEX32.match(r["customer_id"]) for r in history["raw_olist_customers"])
    assert all(HEX32.match(r["order_id"]) and HEX32.match(r["customer_id"]) for r in history["raw_olist_orders"])
    assert all(r["order_status"] in ("processing", "shipped", "delivered") for r in history["raw_olist_orders"])
    assert all(r["review_id"] for r in history["raw_olist_order_reviews"])
    assert all(r["event_id"] for r in history["raw_carrier_tracking_events"] + history["raw_web_events"])
    assert len({r["event_id"] for r in history["raw_web_events"]}) == len(history["raw_web_events"])
    assert len({r["event_id"] for r in history["raw_carrier_tracking_events"]}) == len(history["raw_carrier_tracking_events"])


def test_payments_unique_and_referential(history):
    keys = [(p["order_id"], p["payment_sequential"]) for p in history["raw_olist_order_payments"]]
    assert len(keys) == len(set(keys))
    orders = {r["order_id"] for r in history["raw_olist_orders"]}
    assert {p["order_id"] for p in history["raw_olist_order_payments"]} <= orders
    assert {r["order_id"] for r in history["raw_olist_order_reviews"]} <= orders
    assert {json.loads(json.dumps(r["payload"]))["order_ref"] for r in history["raw_carrier_tracking_events"]} <= orders
    customers = {r["customer_id"] for r in history["raw_olist_customers"]}
    assert {r["customer_id"] for r in history["raw_olist_orders"]} == customers
    per_order = Counter(p["order_id"] for p in history["raw_olist_order_payments"])
    assert set(per_order.values()) == {1, 2}
    uniques = Counter(r["customer_unique_id"] for r in history["raw_olist_customers"])
    assert max(uniques.values()) > 1          # returning customers exist


def test_carrier_scans_follow_the_parcel(history):
    by_order = {}
    for r in history["raw_carrier_tracking_events"]:
        p = r["payload"]
        by_order.setdefault(p["order_ref"], []).append((p["event_time"], p["event"], r["_ingested_at"]))
    done = [v for v in by_order.values() if any(e == "delivered" for _, e, _ in v)]
    assert done
    for scans in done:
        scans.sort()
        assert scans[0][1] == "picked_up" and scans[-1][1] == "delivered" and 2 <= len(scans) <= 4
        assert [s[2] for s in scans] == sorted(s[2] for s in scans)          # arrival order = event order
    delivered_orders = {r["order_id"]: r for r in history["raw_olist_orders"] if r["order_status"] == "delivered"}
    for order_id, row in delivered_orders.items():
        scan_time = [s[0] for s in by_order[order_id] if s[1] == "delivered"][0]
        assert row["order_delivered_customer_date"].strftime("%Y-%m-%d %H:%M:%S") == scan_time


def test_web_checkouts_match_orders(history):
    orders = {r["customer_id"]: r["order_purchase_timestamp"] for r in history["raw_olist_orders"]
              if r["order_status"] == "processing"}
    checkouts = [r for r in history["raw_web_events"] if r["event_type"] == "checkout"]
    assert len(checkouts) == len(orders)
    for c in checkouts:
        assert abs((orders[c["customer_ref"]] - c["event_ts"]).total_seconds()) < 600
    assert {r["event_type"] for r in history["raw_web_events"]} == {"page_view", "add_to_cart", "checkout"}


def test_reviews_follow_delivery_and_share(history):
    delivered = {r["order_id"]: r["order_delivered_customer_date"] for r in history["raw_olist_orders"]
                 if r["order_status"] == "delivered"}
    reviews = history["raw_olist_order_reviews"]
    assert all(r["review_creation_date"] > delivered[r["order_id"]] for r in reviews)
    assert 0.4 < len(reviews) / len(delivered) < 0.75        # the window cuts off the last reviews
    assert {r["review_score"] for r in reviews} <= {1, 2, 3, 4, 5}


def test_ingested_at_is_the_simulated_day():
    day = date(2026, 3, 5)
    for rows in generate_day(day, None, SEED, first_day=date(2026, 1, 1)).values():
        assert rows and all(r["_ingested_at"].date() == day for r in rows)


def test_first_day_cuts_events_of_orders_that_do_not_exist():
    day = date(2026, 3, 5)
    with_history = generate_day(day, None, SEED)
    without = generate_day(day, None, SEED, first_day=day)
    assert len(without["raw_carrier_tracking_events"]) == 0
    assert len(with_history["raw_carrier_tracking_events"]) > 0


def test_carrier_v1_v2_shapes():
    day = date(2026, 3, 5)
    v1 = generate_day(day, None, SEED)["raw_carrier_tracking_events"]
    assert all(set(r["payload"]) == {"shipment_id", "order_ref", "event", "event_time"} for r in v1)
    cond = Conditions.default().with_values("carrier", format="v2")
    v2 = generate_day(day, cond, SEED)["raw_carrier_tracking_events"]
    assert all(set(r["payload"]) == {"shipmentId", "orderReference", "status", "eventTime"} for r in v2)
    assert {r["payload"]["status"]["code"] for r in v2} <= {"PU", "IT", "DL"}
    # same scans, v2 time is UTC = local + 3h
    local = {r["event_id"]: datetime.strptime(r["payload"]["event_time"], "%Y-%m-%d %H:%M:%S") for r in v1}
    for r in v2:
        utc = datetime.strptime(r["payload"]["eventTime"], "%Y-%m-%dT%H:%M:%SZ")
        assert utc - local[r["event_id"]] == timedelta(hours=3)


def test_format_from_switches_on_the_day():
    cond = Conditions.default().with_values("carrier", format_from="2026-03-05")
    before = generate_day(date(2026, 3, 4), cond, SEED)["raw_carrier_tracking_events"]
    after = generate_day(date(2026, 3, 5), cond, SEED)["raw_carrier_tracking_events"]
    assert all("event" in r["payload"] for r in before)
    assert all("eventTime" in r["payload"] for r in after)


def test_dirty_pct_malforms_customer_ids_in_orders_only():
    day = date(2026, 3, 5)
    base = generate_day(day, None, SEED)["raw_olist_orders"]
    out = generate_day(day, Conditions.default().with_values("orders", dirty_pct=30), SEED)
    bad = [r for r in out["raw_olist_orders"] if not HEX32.match(r["customer_id"])]
    assert 0 < len(bad) < len(base)
    assert all(HEX32.match(r["customer_id"]) for r in out["raw_olist_customers"])
    assert not [r for r in base if not HEX32.match(r["customer_id"])]


def test_bad_rows_and_duplicates():
    day = date(2026, 3, 5)
    base = generate_day(day, None, SEED)
    cond = Conditions.default().with_values("orders", bad_rows=3).with_values("payments", duplicate_pct=100)
    out = generate_day(day, cond, SEED)
    assert sum(r["order_id"] is None for r in out["raw_olist_orders"]) == 3
    assert len(out["raw_olist_order_payments"]) == 2 * len(base["raw_olist_order_payments"])
    keys = [(p["order_id"], p["payment_sequential"]) for p in out["raw_olist_order_payments"]]
    assert len(keys) > len(set(keys))
    assert len(out["raw_olist_orders"]) == len(base["raw_olist_orders"]) + 3   # other feeds untouched


def test_carrier_bad_rows_have_no_event_id():
    day = date(2026, 3, 5)
    base = generate_day(day, None, SEED)["raw_carrier_tracking_events"]
    for fmt in ("v1", "v2"):
        cond = Conditions.default().with_values("carrier", bad_rows=4, format=fmt)
        out = generate_day(day, cond, SEED)["raw_carrier_tracking_events"]
        assert sum(r["event_id"] is None for r in out) == 4
        assert sum(r["event_id"] is not None for r in out) == len(base)
        assert all(r["payload"] for r in out)
    assert generate_day(day, None, SEED)["raw_olist_orders"] == generate_day(
        day, Conditions.default().with_values("carrier", bad_rows=4), SEED)["raw_olist_orders"]


def test_bots_add_one_crawler_visitor():
    day = date(2026, 3, 5)
    base = generate_day(day, None, SEED)["raw_web_events"]
    out = generate_day(day, Conditions.default().with_values("web", bots=500), SEED)["raw_web_events"]
    assert len(out) == len(base) + 500
    counts = Counter(r["visitor_id"] for r in out)
    assert counts.most_common(1)[0][1] == 500


def test_late_days_shift_arrival_and_the_format_follows_the_arrival_day():
    day = date(2026, 3, 5)
    base = generate_day(day, None, SEED)["raw_olist_order_reviews"]
    cond = Conditions.default().with_values("reviews", late_days=2)
    late = generate_day(day, cond, SEED)["raw_olist_order_reviews"]
    assert [r["_ingested_at"] - b["_ingested_at"] for r, b in zip(late, base)] == [timedelta(days=2)] * len(base)
    # sent on 4 March but arriving on 5 March: the new format applies
    cond = Conditions.default().with_values("carrier", format_from="2026-03-05", late_days=1)
    scans = generate_day(date(2026, 3, 4), cond, SEED)["raw_carrier_tracking_events"]
    assert scans and all("eventTime" in r["payload"] for r in scans)
    assert all(r["_ingested_at"].date() == date(2026, 3, 5) for r in scans)


def test_arrivals_is_deterministic_half_open_and_additive():
    cond = Conditions.default().with_values("payments", duplicate_pct=30)
    a, b, c = (datetime(2026, 3, 5, h) for h in (0, 9, 15))
    whole = arrivals(cond, SEED, FIRST, a, c)
    assert whole == arrivals(cond, SEED, FIRST, a, c)
    parts = arrivals(cond, SEED, FIRST, a, b)
    for table, rows in arrivals(cond, SEED, FIRST, b, c).items():
        parts[table] = parts[table] + rows
    assert {t: sorted(map(repr, r)) for t, r in whole.items()} == {t: sorted(map(repr, r)) for t, r in parts.items()}
    assert all(a <= r["_ingested_at"] < c for rows in whole.values() for r in rows)
    assert not any(arrivals(cond, SEED, FIRST, c, c).values())


def test_arrivals_with_late_feed_looks_back():
    cond = Conditions.default().with_values("orders", late_days=3)
    start, end = datetime(2026, 3, 10), datetime(2026, 3, 11)
    rows = arrivals(cond, SEED, FIRST, start, end, ["orders"])["raw_olist_orders"]
    assert rows and all(start <= r["_ingested_at"] < end for r in rows)
    assert set(arrivals(cond, SEED, FIRST, start, end, ["orders"])) == {"raw_olist_orders"}


def test_condition_validation():
    assert validate_feed("orders", {"dirty_pct": "12.5", "bad_rows": 2}) == {"dirty_pct": 12.5, "bad_rows": 2}
    assert validate_feed("carrier", {"format_from": date(2026, 1, 2)}) == {"format_from": "2026-01-02"}
    for feed, values in [("orders", {"bots": 1}), ("web", {"bots": -1}), ("carrier", {"format": "v3"}),
                         ("orders", {"dirty_pct": 101}), ("nope", {}), ("web", {"late_days": 1.5}),
                         ("carrier", {"format_from": "yesterday"}), ("web", {"paused": "yes"})]:
        with pytest.raises(ValueError):
            validate_feed(feed, values)
    cond = Conditions.default().with_values("web", bots=3)
    assert cond.to_dict() == {"web": {"bots": 3}}
    assert Conditions.from_dict(cond.to_dict()).get("web").bots == 3
    assert cond.reset("web").to_dict() == {}
