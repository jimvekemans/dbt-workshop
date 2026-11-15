import json
import re
import sys
import threading
import time
from datetime import date, datetime, timedelta
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from studio.feeder import Feeder  # noqa: E402
from studio.feeder.conditions import FEED_TABLES  # noqa: E402
from studio.feeder.writer import ALL_TABLES, COLUMNS  # noqa: E402

NOW = datetime(2026, 6, 15, 14, 30, 0)
TODAY = NOW.date()
HEX32 = re.compile(r"^[0-9a-f]{32}$")


class Clock:
    def __init__(self, now: datetime):
        self.now = now

    def __call__(self) -> datetime:
        return self.now


class FakeDb:
    """Stands in for SingleStore: keeps the rows per (env, table) and understands the writer's statements."""

    def __init__(self):
        self.rows: dict[tuple[str, str], list[tuple]] = {}
        self.statements: list[tuple[str, list]] = []
        self.fail = False

    def table(self, env, name):
        return self.rows.setdefault((env, name), [])

    def stamps(self, env, name):
        return [r[-1] for r in self.table(env, name)]

    def connect(self, env):
        db = self

        class Cursor:
            def __enter__(self): return self
            def __exit__(self, *a): return False

            def execute(self, sql, params=None):
                if db.fail:
                    raise RuntimeError("database is down")
                db.statements.append((env + ":" + sql, list(params or [])))
                table = sql.split("`")[-2] if sql.startswith("SELECT") else sql.split("`")[1]
                self.table, self.sql = table, sql
                if sql.startswith("TRUNCATE"):
                    db.rows[(env, table)] = []
                elif sql.startswith("DELETE"):
                    db.rows[(env, table)] = [r for r in db.table(env, table) if r[-1] < params[0]]
                elif sql.startswith("INSERT"):
                    n_cols = len(COLUMNS[table])
                    db.table(env, table).extend(tuple(params[i:i + n_cols]) for i in range(0, len(params), n_cols))

            def fetchone(self):
                if self.sql.startswith("SELECT MAX"):
                    return (max(db.stamps(env, self.table), default=None),)
                if self.sql.startswith("SELECT MIN"):
                    return (min(db.stamps(env, self.table), default=None),)
                return (len(db.table(env, self.table)),)

        class Conn:
            def cursor(self): return Cursor()
            def commit(self): pass
            def close(self): pass

        return Conn()


@pytest.fixture()
def db():
    return FakeDb()


@pytest.fixture()
def clock():
    return Clock(NOW)


@pytest.fixture()
def feeder(db, tmp_path, clock):
    feeder = Feeder(db.connect, str(tmp_path / "feeder.json"), seed=5, now=clock)
    feeder.seeded_min_days = 0   # the tests seed a few days (reviews trail their orders), the real baseline 90
    return feeder


def count(db, env, feed):
    return len(db.table(env, FEED_TABLES[feed]))


def snapshot(db, env, feed):
    return sorted(db.table(env, FEED_TABLES[feed]), key=repr)


def newest(db, env, feed):
    return max(db.stamps(env, FEED_TABLES[feed]))


def all_stamps(db, env):
    return [t for feed in FEED_TABLES for t in db.stamps(env, FEED_TABLES[feed])]


def test_seed_fills_all_tables_with_nothing_after_now(feeder, db):
    said = []
    totals = feeder.seed("val", days=20, progress=said.append)
    assert set(totals) == set(ALL_TABLES) and all(n > 0 for n in totals.values())
    assert feeder.is_seeded("val") and not feeder.is_seeded("dev")
    assert feeder.today("val") == TODAY
    assert said and all(s.endswith(".") for s in said)
    stamps = all_stamps(db, "val")
    assert max(stamps) <= NOW and min(stamps) >= datetime.combine(TODAY - timedelta(days=20), datetime.min.time())
    assert feeder.status()["rows"]["val"]["raw_olist_orders"] == totals["raw_olist_orders"]
    first = snapshot(db, "val", "orders")
    assert feeder.seed("val", days=20) == totals                 # reseeding gives exactly the same rows
    assert snapshot(db, "val", "orders") == first


def test_seed_subset_of_feeds(feeder, db):
    feeder.seed("dev", days=5)
    before = count(db, "dev", "orders")
    feeder.seed("dev", days=5, feeds=["web"])
    assert count(db, "dev", "orders") == before


def test_today_is_the_real_date_and_follows_the_clock(feeder, clock):
    assert feeder.today("dev") == TODAY and feeder.today("prd") == TODAY
    clock.now = NOW + timedelta(days=2)
    assert feeder.today("dev") == TODAY + timedelta(days=2)
    status = feeder.status()
    assert set(status["simulated_day"].values()) == {(TODAY + timedelta(days=2)).isoformat()}


def test_status_reports_the_newest_arrival_per_feed(feeder, db):
    feeder.seed("dev", days=10)
    newest_seen = feeder.status()["newest"]["dev"]
    assert set(newest_seen) == set(FEED_TABLES)
    assert newest_seen["orders"] == newest(db, "dev", "orders").isoformat()


def test_deliver_replays_the_window_and_is_idempotent(feeder, db):
    feeder.seed("dev", days=10)
    before = snapshot(db, "dev", "orders")
    first = feeder.deliver("dev", days=3)
    after_one = snapshot(db, "dev", "orders")
    assert first["raw_olist_orders"] > 0
    assert after_one == before                                   # healthy conditions: the same rows again
    feeder.deliver("dev", days=3)
    assert snapshot(db, "dev", "orders") == after_one
    assert feeder.status()["last_delivery"]
    assert feeder.status()["rows"]["dev"]["raw_olist_orders"] == count(db, "dev", "orders")


def test_deliver_only_touches_the_chosen_feeds(feeder, db):
    feeder.seed("dev", days=5)
    feeder.set_condition("dev", "web", duplicate_pct=100)
    orders = snapshot(db, "dev", "orders")
    out = feeder.deliver("dev", days=2, feeds=["web"])
    assert set(out) == {"raw_web_events"}
    assert snapshot(db, "dev", "orders") == orders
    assert count(db, "dev", "web") > count(db, "dev", "customers")   # duplicated rows in the window


def test_paused_feed_has_nothing_in_the_window_and_is_late(feeder, db):
    feeder.seed("dev", days=10)
    feeder.set_condition("dev", "carrier", paused=True)
    feeder.deliver("dev", days=3)
    window_start = datetime.combine(TODAY - timedelta(days=2), datetime.min.time())
    assert newest(db, "dev", "carrier") < window_start
    age = NOW - newest(db, "dev", "carrier")
    assert timedelta(days=2) < age <= timedelta(days=3, hours=14, minutes=30)
    assert newest(db, "dev", "orders") > window_start             # the other feeds are fine
    feeder.set_condition("dev", "carrier", paused=False)
    feeder.deliver("dev", days=1)                                   # only today comes back
    assert NOW - newest(db, "dev", "carrier") < timedelta(hours=14, minutes=30)
    feeder.deliver("dev", days=3)
    assert count(db, "dev", "carrier") == len(snapshot(db, "dev", "carrier"))
    assert newest(db, "dev", "carrier") > window_start


def test_late_days_shifts_arrivals_and_leaves_nothing_in_the_future(feeder, db):
    feeder.seed("dev", days=10)
    healthy = [r for r in db.table("dev", FEED_TABLES["reviews"])]
    feeder.set_condition("dev", "reviews", late_days=2)
    feeder.deliver("dev", days=4)
    window_start = datetime.combine(TODAY - timedelta(days=3), datetime.min.time())
    late = db.table("dev", FEED_TABLES["reviews"])
    assert max(r[-1] for r in late) <= NOW
    ids_in_window = {r[0]: r[-1] for r in late if r[-1] >= window_start}
    healthy_by_id = {r[0]: r[-1] for r in healthy}
    shifted = [i for i in ids_in_window if i in healthy_by_id and healthy_by_id[i] < window_start]
    assert not shifted or all(ids_in_window[i] - healthy_by_id[i] == timedelta(days=2) for i in shifted)
    # a review that arrives two days late is not there yet when it would arrive after now
    assert len(late) < len(healthy) or newest(db, "dev", "reviews") <= NOW
    assert NOW - max(r[-1] for r in late) >= timedelta(0)


def test_late_days_exact_shift(feeder, db):
    feeder.seed("dev", days=10)
    feeder.set_condition("dev", "payments", late_days=1)
    feeder.deliver("dev", days=10)
    shifted = list(db.table("dev", FEED_TABLES["payments"]))
    window_start = datetime.combine(TODAY - timedelta(days=9), datetime.min.time())
    feeder.reset_conditions("dev")
    feeder.deliver("dev", days=10)
    plain = db.table("dev", FEED_TABLES["payments"])
    by_key = {(r[0], r[1]): r[-1] for r in plain}
    for r in shifted:
        key = (r[0], r[1])
        if key in by_key and r[-1] >= window_start:
            assert r[-1] - by_key[key] == timedelta(days=1)
    assert max(r[-1] for r in shifted) <= NOW


def test_duplicates_dirty_bad_rows_bots_apply_to_the_window(feeder, db):
    feeder.seed("dev", days=6)
    plain = {feed: count(db, "dev", feed) for feed in ("payments", "orders", "web", "carrier")}
    feeder.set_condition("dev", "payments", duplicate_pct=100)
    feeder.set_condition("dev", "orders", dirty_pct=100, bad_rows=3)
    feeder.set_condition("dev", "web", bots=50)
    feeder.set_condition("dev", "carrier", bad_rows=2)
    feeder.deliver("dev", days=2)
    window_start = datetime.combine(TODAY - timedelta(days=1), datetime.min.time())
    payments = [r for r in db.table("dev", FEED_TABLES["payments"]) if r[-1] >= window_start]
    keys = [(r[0], r[1]) for r in payments]
    assert len(keys) > len(set(keys))
    orders = [r for r in db.table("dev", FEED_TABLES["orders"]) if r[-1] >= window_start]
    assert sum(r[0] is None for r in orders) == 6 and all(not HEX32.match(r[1]) or r[0] is None for r in orders)
    old = [r for r in db.table("dev", FEED_TABLES["orders"]) if r[-1] < window_start]
    assert all(HEX32.match(r[1]) for r in old)                      # outside the window nothing changes
    assert count(db, "dev", "web") > plain["web"]
    scans = [r for r in db.table("dev", FEED_TABLES["carrier"]) if r[-1] >= window_start]
    assert sum(r[0] is None for r in scans) == 2 * 2
    feeder.reset_conditions("dev")
    feeder.deliver("dev", days=2)
    assert {feed: count(db, "dev", feed) for feed in plain} == plain


def test_carrier_format_follows_the_arrival_day(feeder, db):
    feeder.seed("dev", days=10)
    switch = (TODAY - timedelta(days=2)).isoformat()
    feeder.set_condition("dev", "carrier", format_from=switch)
    feeder.deliver("dev", days=5)
    switch_at = datetime.combine(date.fromisoformat(switch), datetime.min.time())
    for event_id, payload, at in db.table("dev", FEED_TABLES["carrier"]):
        if at >= datetime.combine(TODAY - timedelta(days=4), datetime.min.time()):
            text = payload if isinstance(payload, str) else json.dumps(payload)
            assert ("eventTime" in text) == (at >= switch_at), (at, text)


def test_nothing_is_ever_stamped_after_now(feeder, db, clock):
    feeder.seed("dev", days=8)
    feeder.set_condition("dev", "orders", late_days=3, duplicate_pct=50)
    feeder.set_condition("dev", "web", bots=20)
    feeder.deliver("dev", days=5)
    clock.now = NOW + timedelta(hours=3)
    feeder.tick("dev")
    assert max(all_stamps(db, "dev")) <= clock.now


def test_conditions_persist_and_reset(feeder, db, tmp_path, clock):
    feeder.set_condition("val", "carrier", format="v2", format_from="2026-06-10")
    feeder.set_condition("val", "orders", dirty_pct=10)
    assert feeder.status()["conditions"]["val"] == {"carrier": {"format": "v2", "format_from": "2026-06-10"},
                                                   "orders": {"dirty_pct": 10}}
    again = Feeder(db.connect, str(tmp_path / "feeder.json"), seed=5, now=clock)
    assert again.status()["conditions"]["val"]["orders"] == {"dirty_pct": 10}
    again.reset_conditions("val", "orders")
    assert "orders" not in again.status()["conditions"]["val"]
    again.reset_conditions("val")
    assert again.status()["conditions"]["val"] == {}
    with pytest.raises(ValueError):
        again.set_condition("val", "web", bots=-5)


def test_tick_delivers_only_what_arrived_since_the_last_round_and_matches_a_reseed(feeder, db, clock):
    feeder.seed("dev", days=10)
    seeded = count(db, "dev", "web")
    for minutes in (20, 45, 90, 400):                              # 14:50, 15:15, 16:00, 21:10
        clock.now = NOW + timedelta(minutes=minutes)
        feeder.tick("dev")
    assert max(all_stamps(db, "dev")) <= clock.now
    assert count(db, "dev", "web") > seeded
    ticked = {feed: snapshot(db, "dev", feed) for feed in FEED_TABLES}
    feeder.seed("dev", days=10)                                    # reseeding gives the same rows
    for feed in FEED_TABLES:
        assert snapshot(db, "dev", feed) == ticked[feed], feed
    before = count(db, "dev", "orders")
    feeder.tick("dev")                                             # no time has passed: nothing new
    assert count(db, "dev", "orders") == before


def test_tick_over_midnight_and_a_paused_feed(feeder, db, clock):
    feeder.seed("dev", days=5)
    feeder.set_condition("dev", "carrier", paused=True)
    clock.now = datetime.combine(TODAY + timedelta(days=1), datetime.min.time()) + timedelta(hours=10)
    feeder.tick("dev")
    assert newest(db, "dev", "orders") > datetime.combine(TODAY + timedelta(days=1), datetime.min.time())
    assert newest(db, "dev", "carrier") <= NOW                      # the paused feed delivered nothing
    assert max(all_stamps(db, "dev")) <= clock.now


def test_tick_does_not_repeat_a_replayed_window(feeder, db, clock):
    feeder.seed("dev", days=5)
    feeder.deliver("dev", days=2)
    before = count(db, "dev", "orders")
    feeder.tick("dev")
    assert count(db, "dev", "orders") == before
    clock.now = NOW + timedelta(hours=2)
    feeder.tick("dev")
    after = snapshot(db, "dev", "orders")
    feeder.deliver("dev", days=2)
    assert snapshot(db, "dev", "orders") == after


def test_old_stacks_with_future_rows_are_not_seeded(feeder, db):
    feeder.seed("dev", days=5)
    assert feeder.is_seeded("dev")
    db.table("dev", FEED_TABLES["orders"]).append(("x",) * 8 + (NOW + timedelta(days=3),))
    assert not feeder.is_seeded("dev")


def test_all_statements_are_parametrised(feeder, db):
    feeder.seed("dev", days=3)
    feeder.deliver("dev", days=2)
    for sql, params in db.statements:
        assert "'" not in sql
        assert "TRUNCATE" in sql or "SELECT" in sql or sql.count("%s") == len(params)


def test_background_ticks_seeded_envs_and_never_raises(feeder, db, clock):
    feeder.seed("dev", days=5)
    before = count(db, "dev", "web")
    clock.now = NOW + timedelta(hours=4)
    feeder.start_background(0.05, lambda: False)
    time.sleep(0.4)
    assert feeder.status()["running"]
    db.fail = True
    time.sleep(0.2)                                        # failures are logged, the thread lives on
    assert feeder.status()["running"]
    db.fail = False
    feeder.stop()
    assert not feeder.status()["running"]
    assert count(db, "dev", "web") > before and count(db, "val", "web") == 0


def test_background_respects_pause(feeder, db, clock):
    feeder.seed("dev", days=5)
    before = count(db, "dev", "web")
    clock.now = NOW + timedelta(hours=4)
    feeder.start_background(0.05, lambda: True)
    time.sleep(0.3)
    feeder.stop()
    assert count(db, "dev", "web") == before


def test_thread_safety_of_concurrent_deliveries(feeder, db):
    feeder.seed("dev", days=5)
    before = snapshot(db, "dev", "orders")
    threads = [threading.Thread(target=feeder.deliver, args=("dev", 2)) for _ in range(5)]
    [t.start() for t in threads]
    [t.join() for t in threads]
    assert snapshot(db, "dev", "orders") == before


def test_rows_of_today_alone_do_not_count_as_the_baseline(feeder, db):
    """After a stack reset the background feeder may add today's rows to the emptied tables before the baseline
    check runs: those rows must not make the environment look seeded."""
    feeder.seed("dev", days=20)
    feeder.seeded_min_days = 3
    assert feeder.is_seeded("dev")
    for table in ALL_TABLES:
        db.rows[("dev", table)] = [r for r in db.table("dev", table) if r[-1] >= NOW - timedelta(hours=2)][:2] \
            or db.table("dev", table)[-1:]
    assert all(db.table("dev", t) for t in ALL_TABLES)
    newest_only = all(min(db.stamps("dev", t)) > NOW - timedelta(days=3) for t in ALL_TABLES)
    assert newest_only and not feeder.is_seeded("dev")
