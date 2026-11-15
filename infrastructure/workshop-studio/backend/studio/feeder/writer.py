"""Writes generated rows into the landing tables with batched, parametrised multi-row INSERTs (PyMySQL)."""
from __future__ import annotations

import json
from datetime import datetime
from typing import Any, Callable, Iterable

import pymysql

from .conditions import FEED_TABLES

COLUMNS: dict[str, tuple[str, ...]] = {
    "raw_olist_customers": ("customer_id", "customer_unique_id", "customer_zip_code_prefix", "customer_city",
                            "customer_state", "_ingested_at"),
    "raw_olist_orders": ("order_id", "customer_id", "order_status", "order_purchase_timestamp", "order_approved_at",
                         "order_delivered_carrier_date", "order_delivered_customer_date",
                         "order_estimated_delivery_date", "_ingested_at"),
    "raw_olist_order_payments": ("order_id", "payment_sequential", "payment_type", "payment_installments",
                                 "payment_value", "_ingested_at"),
    "raw_olist_order_reviews": ("review_id", "order_id", "review_score", "review_comment_title",
                                "review_comment_message", "review_creation_date", "review_answer_timestamp",
                                "_ingested_at"),
    "raw_carrier_tracking_events": ("event_id", "payload", "_ingested_at"),
    "raw_web_events": ("event_id", "visitor_id", "customer_ref", "event_type", "page", "referrer", "event_ts",
                       "_ingested_at"),
}
ALL_TABLES = tuple(FEED_TABLES.values())
ENVIRONMENTS = ("dev", "val", "prd")
BATCH_ROWS = 500


def _check_env(env: str) -> None:
    if env not in ENVIRONMENTS:
        raise ValueError(f"Unknown environment '{env}'.")


def _check_table(table: str) -> None:
    if table not in COLUMNS:
        raise ValueError(f"Unknown landing table '{table}'.")


def _value(column: str, value: Any) -> Any:
    if column == "payload" and not isinstance(value, (str, bytes)):
        return json.dumps(value, separators=(",", ":"))
    if isinstance(value, datetime):
        return value.replace(microsecond=0)
    return value


SEEDED_MIN_DAYS = 30   # the baseline holds 90 days; a table whose oldest row is younger was never seeded
BUILT_TABLES_MIN = 5   # a full build makes 7 to 9 tables per database; fewer means a build never finished


class Writer:
    """`connect(env)` returns a PyMySQL-style connection to `<env>_landing` (root)."""

    def __init__(self, connect: Callable[[str], Any]):
        self.connect = connect

    def insert(self, env: str, rows_by_table: dict[str, list[dict[str, Any]]], batch: int = BATCH_ROWS) -> dict[str, int]:
        """Insert rows; returns rows written per table."""
        _check_env(env)
        written: dict[str, int] = {}
        todo = {t: r for t, r in rows_by_table.items() if r}
        if not todo:
            return written
        conn = self.connect(env)
        try:
            for table, rows in todo.items():
                _check_table(table)
                cols = COLUMNS[table]
                head = f"INSERT INTO `{table}` ({', '.join('`' + c + '`' for c in cols)}) VALUES "
                one = "(" + ", ".join(["%s"] * len(cols)) + ")"
                with conn.cursor() as cur:
                    for start in range(0, len(rows), batch):
                        chunk = rows[start:start + batch]
                        params: list[Any] = []
                        for row in chunk:
                            params.extend(_value(c, row.get(c)) for c in cols)
                        cur.execute(head + ", ".join([one] * len(chunk)), params)
                written[table] = len(rows)
            conn.commit()
        finally:
            conn.close()
        return written

    def replace_since(self, env: str, rows_by_table: dict[str, list[dict[str, Any]]], since: datetime,
                      batch: int = BATCH_ROWS) -> dict[str, int]:
        """In one transaction per table set: delete the rows that arrived on or after `since`, insert the new rows."""
        _check_env(env)
        conn = self.connect(env)
        written: dict[str, int] = {}
        try:
            for table, rows in rows_by_table.items():
                _check_table(table)
                cols = COLUMNS[table]
                with conn.cursor() as cur:
                    cur.execute(f"DELETE FROM `{table}` WHERE `_ingested_at` >= %s", [since.replace(microsecond=0)])
                    head = f"INSERT INTO `{table}` ({', '.join('`' + c + '`' for c in cols)}) VALUES "
                    one = "(" + ", ".join(["%s"] * len(cols)) + ")"
                    for start in range(0, len(rows), batch):
                        chunk = rows[start:start + batch]
                        params: list[Any] = []
                        for row in chunk:
                            params.extend(_value(c, row.get(c)) for c in cols)
                        cur.execute(head + ", ".join([one] * len(chunk)), params)
                written[table] = len(rows)
            conn.commit()
        finally:
            conn.close()
        return written

    def newest(self, env: str, tables: Iterable[str] = ALL_TABLES) -> dict[str, datetime | None]:
        """The latest arrival time per table (None for an empty table)."""
        _check_env(env)
        conn = self.connect(env)
        try:
            out: dict[str, datetime | None] = {}
            with conn.cursor() as cur:
                for table in tables:
                    _check_table(table)
                    cur.execute(f"SELECT MAX(`_ingested_at`) FROM `{table}`")
                    out[table] = cur.fetchone()[0]
            return out
        finally:
            conn.close()

    def oldest(self, env: str, tables: Iterable[str] = ALL_TABLES) -> dict[str, datetime | None]:
        """The earliest arrival time per table (None for an empty table)."""
        _check_env(env)
        conn = self.connect(env)
        try:
            out: dict[str, datetime | None] = {}
            with conn.cursor() as cur:
                for table in tables:
                    _check_table(table)
                    cur.execute(f"SELECT MIN(`_ingested_at`) FROM `{table}`")
                    out[table] = cur.fetchone()[0]
            return out
        finally:
            conn.close()

    @staticmethod
    def _built_sql(env: str) -> str:
        """True when the environment's working and report tables exist. Not the run logbook: only Airflow runs write
        to it, so an environment the studio built itself (production) would look unbuilt for ever."""
        return " AND ".join(
            f"(SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = '{env}_{db}') >= {BUILT_TABLES_MIN}"
            for db in ("edw_dv_bdv", "im_dimensional")
        )

    def tables_built(self, env: str) -> bool:
        _check_env(env)
        conn = self.connect(env)
        try:
            with conn.cursor() as cur:
                cur.execute(f"SELECT {self._built_sql(env)}")
                row = cur.fetchone()
                return bool(row and row[0])
        finally:
            conn.close()

    def quick_ready(self, env: str) -> bool:
        """One read-only round trip: every landing table has a row and the environment's tables are built."""
        _check_env(env)
        # "has the baseline", not "has a row": the background feeder may already have added today's rows to a
        # table that was emptied by a stack reset, and those few rows must not pass for the baseline.
        landing = " AND ".join(
            f"EXISTS (SELECT 1 FROM `{t}` WHERE `_ingested_at` < NOW() - INTERVAL {SEEDED_MIN_DAYS} DAY)" for t in ALL_TABLES
        )
        built = self._built_sql(env)
        conn = self.connect(env)
        try:
            with conn.cursor() as cur:
                try:
                    cur.execute(f"SELECT ({landing}) AND {built}")
                except pymysql.MySQLError:   # a table or database is missing
                    return False
                row = cur.fetchone()
                return bool(row and row[0])
        finally:
            conn.close()

    def truncate(self, env: str, tables: Iterable[str]) -> None:
        _check_env(env)
        conn = self.connect(env)
        try:
            with conn.cursor() as cur:
                for table in tables:
                    _check_table(table)
                    cur.execute(f"TRUNCATE TABLE `{table}`")
            conn.commit()
        finally:
            conn.close()

    def counts(self, env: str, tables: Iterable[str] = ALL_TABLES) -> dict[str, int]:
        _check_env(env)
        conn = self.connect(env)
        try:
            out: dict[str, int] = {}
            with conn.cursor() as cur:
                for table in tables:
                    _check_table(table)
                    cur.execute(f"SELECT COUNT(*) FROM `{table}`")
                    out[table] = int(cur.fetchone()[0])
            return out
        finally:
            conn.close()
