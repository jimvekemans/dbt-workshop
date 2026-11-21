"""The background data feeder: the only thing that fills the landing tables of the workshop stack.

"Today" is the real current date, and no row is ever stamped after the current time. Seeding writes the 90 full days
before today plus today up to now. `deliver(days=n)` lets the last n days (today is day 1) happen again under the
conditions set now. The background thread adds, every few seconds, the rows of today that arrived since its last round.
All rows come from the deterministic generator, so reseeding gives exactly what the background rounds have delivered.
"""
from __future__ import annotations

import json
import logging
import os
import threading
from datetime import date, datetime, time, timedelta
from typing import Any, Callable

from ..names import ENV_WORDS
from .conditions import FEED_TABLES, FEEDS, Conditions
from .generator import arrivals
from .writer import ALL_TABLES, ENVIRONMENTS, SEEDED_MIN_DAYS, Writer

__all__ = ["Feeder", "Conditions", "FEEDS", "ENVIRONMENTS"]

log = logging.getLogger("studio.feeder")


def _tables(feeds: list[str] | tuple[str, ...] | None) -> list[str]:
    chosen = list(FEEDS) if feeds is None else list(feeds)
    for feed in chosen:
        if feed not in FEEDS:
            raise ValueError(f"Unknown feed '{feed}'. Choose one of: {', '.join(FEEDS)}.")
    return [FEED_TABLES[f] for f in chosen]


def _midnight(day: date) -> datetime:
    return datetime.combine(day, time.min)


class Feeder:
    def __init__(self, connect: Callable[[str], Any], state_path: str, seed: int = 20260101,
                 now: Callable[[], datetime] = datetime.now):
        self.writer = Writer(connect)
        self.state_path = state_path
        self.seed_value = seed
        self.seeded_min_days = SEEDED_MIN_DAYS   # how old the oldest row must be to count as the baseline
        self._clock = now
        self._lock = threading.RLock()
        self._rows: dict[str, dict[str, int]] = {}
        self._newest: dict[str, dict[str, str | None]] = {}
        self._thread: threading.Thread | None = None
        self._stop = threading.Event()
        self._state = self._load()

    # ---- state -------------------------------------------------------------------------------------------------
    def _load(self) -> dict[str, Any]:
        try:
            with open(self.state_path, encoding="utf-8") as fh:
                state = json.load(fh)
            if isinstance(state, dict) and isinstance(state.get("envs"), dict):
                return state
        except (OSError, ValueError):
            pass
        return {"last_delivery": None, "envs": {}}

    def _save(self) -> None:
        folder = os.path.dirname(self.state_path)
        if folder:
            os.makedirs(folder, exist_ok=True)
        tmp = f"{self.state_path}.tmp"
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(self._state, fh, indent=1, sort_keys=True)
        os.replace(tmp, self.state_path)

    def forget(self) -> None:
        """Drop the saved state (conditions, delivery marks): the next seed starts from the defaults."""
        with self._lock:
            self._state = {"last_delivery": None, "envs": {}}
            self._rows, self._newest = {}, {}
            self._save()

    def _env(self, env: str) -> dict[str, Any]:
        if env not in ENVIRONMENTS:
            raise ValueError(f"Unknown environment '{env}'.")
        return self._state["envs"].setdefault(env, {"conditions": {}})

    def _conditions(self, env: str) -> Conditions:
        return Conditions.from_dict(self._env(env).get("conditions"))

    def _now(self) -> datetime:
        return self._clock().replace(microsecond=0)

    def _first_day(self, env: str, today: date) -> date:
        raw = self._env(env).get("first_day")
        return date.fromisoformat(raw) if raw else today - timedelta(days=90)

    # ---- queries -----------------------------------------------------------------------------------------------
    def today(self, env: str | None = None) -> date:
        """The real current date (the same for every environment)."""
        return self._now().date()

    def status(self) -> dict[str, Any]:
        with self._lock:
            today = self.today().isoformat()
            return {
                "running": bool(self._thread and self._thread.is_alive() and not self._stop.is_set()),
                "simulated_day": {env: today for env in ENVIRONMENTS},
                "last_delivery": self._state.get("last_delivery"),
                "conditions": {env: self._conditions(env).to_dict() for env in ENVIRONMENTS},
                "rows": {env: dict(self._rows.get(env, {})) for env in ENVIRONMENTS},
                "newest": {env: dict(self._newest.get(env, {})) for env in ENVIRONMENTS},
            }

    def refresh_rows(self, env: str) -> dict[str, int]:
        counts = self.writer.counts(env)
        newest = self.writer.newest(env)
        with self._lock:
            self._rows[env] = counts
            self._newest[env] = {feed: (newest[FEED_TABLES[feed]].isoformat() if newest.get(FEED_TABLES[feed]) else None)
                                 for feed in FEEDS}
        return counts

    def is_seeded(self, env: str) -> bool:
        """Every table holds the baseline (its oldest row is weeks old, not just today's rows from the background
        feeder after a stack reset) and no row is stamped after the current time (older stacks had future rows)."""
        if not all(n > 0 for n in self.refresh_rows(env).values()):
            return False
        oldest = getattr(self.writer, "oldest", None)
        if oldest is not None:
            before = (self._now() - timedelta(days=self.seeded_min_days)).isoformat()
            if not all(t is not None and t.isoformat() <= before for t in oldest(env).values()):
                return False
        limit = (self._now() + timedelta(minutes=1)).isoformat()
        return all(t is None or t <= limit for t in self._newest[env].values())

    def quick_ready(self, env: str) -> bool:
        """The cheap reality check: data in every landing table and the tables built, in one SQL round trip."""
        return self.writer.quick_ready(env)

    def tables_built(self, env: str) -> bool:
        return self.writer.tables_built(env)

    # ---- conditions --------------------------------------------------------------------------------------------
    def set_condition(self, env: str, feed: str, **values: Any) -> dict[str, Any]:
        with self._lock:
            updated = self._conditions(env).with_values(feed, **values)
            self._env(env)["conditions"] = updated.to_dict()
            self._save()
            return updated.get(feed).to_dict()

    def reset_conditions(self, env: str, feed: str | None = None) -> None:
        with self._lock:
            self._env(env)["conditions"] = self._conditions(env).reset(feed).to_dict()
            self._save()

    # ---- seeding -----------------------------------------------------------------------------------------------
    def seed(self, env: str, days: int = 90, feeds: list[str] | None = None,
             progress: Callable[[str], None] | None = None) -> dict[str, int]:
        """Replace the chosen feeds' tables with the baseline: `days` full days before today plus today until now."""
        say = progress or (lambda text: None)
        chosen = [f for f in FEEDS if FEED_TABLES[f] in _tables(feeds)]
        tables = [FEED_TABLES[f] for f in chosen]
        with self._lock:
            state = self._env(env)
            now = self._now()
            first = now.date() - timedelta(days=days)
            self.writer.truncate(env, tables)
            say(f"De datatabellen van {ENV_WORDS.get(env, env)} zijn leeggemaakt; er worden {days} dagen historiek geschreven.")
            baseline = Conditions.default()
            totals = {t: 0 for t in tables}
            batch: dict[str, list] = {t: [] for t in tables}
            for n in range(days + 1):
                start = _midnight(first + timedelta(days=n))
                for table, rows in arrivals(baseline, self.seed_value, first, start, min(start + timedelta(days=1), now),
                                            chosen).items():
                    batch[table].extend(rows)
                if (n + 1) % 15 == 0 or n == days:
                    for table, count in self.writer.insert(env, batch).items():
                        totals[table] += count
                    batch = {t: [] for t in tables}
                    say(f"{n + 1} van {days + 1} dagen historiek naar {ENV_WORDS.get(env, env)} geschreven.")
            if len(chosen) == len(FEEDS):
                state["first_day"] = first.isoformat()
            else:
                state.setdefault("first_day", first.isoformat())
            state.setdefault("fed_until", {}).update({f: now.isoformat() for f in chosen})
            self._save()
            self.refresh_rows(env)
            return totals

    # ---- delivery ----------------------------------------------------------------------------------------------
    def deliver(self, env: str, days: int = 1, feeds: list[str] | None = None) -> dict[str, int]:
        """Let the last `days` days (today is day 1) happen again under the conditions set now.

        The rows of the chosen feeds that arrived on or after the start of that window are replaced by the rows that
        arrive in the window under the current conditions; the result does not depend on how often this is called.
        """
        if days < 1:
            raise ValueError("days must be at least 1.")
        chosen = [f for f in FEEDS if FEED_TABLES[f] in _tables(feeds)]
        with self._lock:
            state = self._env(env)
            conditions = self._conditions(env)
            now = self._now()
            start = _midnight(now.date() - timedelta(days=days - 1))
            live = [f for f in chosen if not conditions.get(f).paused]   # a paused feed delivers nothing
            rows = arrivals(conditions, self.seed_value, self._first_day(env, now.date()), start, now, live)
            payload = {FEED_TABLES[f]: rows.get(FEED_TABLES[f], []) for f in chosen}
            written = self.writer.replace_since(env, payload, start)
            state.setdefault("fed_until", {}).update({f: now.isoformat() for f in chosen})
            self._state["last_delivery"] = now.isoformat()
            self._save()
            self.refresh_rows(env)
            return written

    def tick(self, env: str) -> dict[str, int]:
        """Deliver the rows that arrived since the last round (today only, unless the stack was off for a while)."""
        with self._lock:
            state = self._env(env)
            conditions = self._conditions(env)
            now = self._now()
            fed = state.setdefault("fed_until", {})
            first = self._first_day(env, now.date())
            payload: dict[str, list] = {}
            for feed in FEEDS:
                since = fed.get(feed)
                if since and not conditions.get(feed).paused:
                    payload.update(arrivals(conditions, self.seed_value, first, datetime.fromisoformat(since), now, [feed]))
                fed[feed] = now.isoformat()
            written = self.writer.insert(env, payload)
            if written:
                self._state["last_delivery"] = now.isoformat()
                cache = self._rows.setdefault(env, {})
                for table, count in written.items():
                    cache[table] = cache.get(table, 0) + count
                newest = self._newest.setdefault(env, {})
                for feed in FEEDS:
                    if payload.get(FEED_TABLES[feed]):
                        newest[feed] = max(r["_ingested_at"] for r in payload[FEED_TABLES[feed]]).isoformat()
            self._save()
            return written

    # ---- background --------------------------------------------------------------------------------------------
    def start_background(self, interval_seconds: float, should_pause: Callable[[], bool] = lambda: False) -> None:
        with self._lock:
            if self._thread and self._thread.is_alive():
                return
            self._stop = threading.Event()
            stop = self._stop
            self._thread = threading.Thread(target=self._loop, args=(interval_seconds, should_pause, stop),
                                            name="studio-feeder", daemon=True)
            self._thread.start()

    def stop(self) -> None:
        with self._lock:
            thread = self._thread
            self._stop.set()
        if thread and thread is not threading.current_thread():
            thread.join(timeout=30)
        with self._lock:
            self._thread = None

    def _loop(self, interval: float, should_pause: Callable[[], bool], stop: threading.Event) -> None:
        log.info("Data feeder started: new rows every %s seconds.", interval)
        while not stop.wait(interval):
            try:
                if should_pause():
                    continue
            except Exception:   # noqa: BLE001 - a broken callback must not end the feeder
                log.exception("Feeder pause check failed; skipping this round.")
                continue
            for env in ENVIRONMENTS:
                if stop.is_set():
                    break
                try:
                    # asked afresh every round: after a stack reset the remembered row counts are wrong, and
                    # rows added to an emptied table would make it look seeded
                    if self.is_seeded(env):
                        self.tick(env)
                except Exception as exc:   # noqa: BLE001
                    log.warning("Could not deliver new data to %s: %s", env, exc)
        log.info("Data feeder stopped.")
