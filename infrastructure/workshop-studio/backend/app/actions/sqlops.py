"""SingleStore actions: channel fault injection, trickle, raw SQL, landing reset."""
from __future__ import annotations

from typing import Any

import pymysql

from .base import Action, ActionError, Ctx, Result, register, require

ENV_DB = {"dev": "analytics_dev", "prod": "analytics_prod"}
CHANNELS = ("webapp", "kafka", "drive", "carrier", "web")   # carrier + web: generated feeds (studio_schema.sql)
CHANNEL_COLUMNS = {"enabled", "delay_batches", "duplicate_pct", "partial_pct", "null_key_pct", "schema_variant",
                   "late_pct", "bot_events"}


def envs(env: str) -> list[str]:
    if env == "both":
        return ["dev", "prod"]
    if env not in ENV_DB:
        raise ActionError(f"env must be dev|prod|both, got {env!r}")
    return [env]


def connect(ctx: Ctx, db: str | None = None):
    s = ctx.settings
    return pymysql.connect(host=s.s2_host, port=s.s2_port, user=s.s2_user, password=s.s2_password,
                           database=db, autocommit=True, connect_timeout=5)


@register
class Sql(Action):
    name = "sql"

    """{action: sql, env, statement, undo?, ignore_errors?: [1917], undo_ignore_errors?: [1917]}. ignore_errors makes a
    statement idempotent where SingleStore has no IF [NOT] EXISTS (GRANT/REVOKE GROUP, DROP ROLE/GROUP): the listed
    MySQL error codes count as done."""

    def render(self, p, ctx):
        require(p, "env", "statement")
        return [f"USE {ENV_DB[p['env']]}; {p['statement']}"]

    def run(self, p, ctx):
        ignore = {int(c) for c in p.get("ignore_errors") or []}
        conn = connect(ctx, ENV_DB[p["env"]])
        try:
            with conn.cursor() as cur:
                try:
                    cur.execute(p["statement"])
                except pymysql.MySQLError as e:
                    if e.args and e.args[0] in ignore:
                        return Result(output=f"already in place (error {e.args[0]} ignored): {e.args[-1]}")
                    raise
                rows = cur.fetchall() if cur.description else []
        finally:
            conn.close()
        return Result(output=str(rows[:20]), data={"rows": rows})


@register
class SetChannel(Action):
    name = "set_channel"

    def _values(self, p: dict[str, Any]) -> dict[str, Any]:
        values = {k: v for k, v in p.items() if k in CHANNEL_COLUMNS}
        if p.get("field") is not None:                   # knob binding: {field: delay_batches, value: 2}
            values[p["field"]] = p["value"]
        bad = set(values) - CHANNEL_COLUMNS
        if bad or not values:
            raise ActionError(f"set_channel needs at least one of {sorted(CHANNEL_COLUMNS)}")
        if "enabled" in values:
            values["enabled"] = 1 if values["enabled"] in (1, True, "1", "true") else 0
        return values

    def render(self, p, ctx):
        require(p, "env", "channel")
        if p["channel"] not in CHANNELS:
            raise ActionError(f"channel must be one of {CHANNELS}")
        sets = ", ".join(f"{k}={v!r}" for k, v in self._values(p).items())
        return [f"UPDATE {ENV_DB[e]}._source_channel_config SET {sets} WHERE channel='{p['channel']}';"
                for e in envs(p["env"])]

    def run(self, p, ctx):
        values = self._values(p)
        conn = connect(ctx)
        try:
            with conn.cursor() as cur:
                for e in envs(p["env"]):
                    sets = ", ".join(f"{k}=%s" for k in values) + ", updated_at=NOW()"
                    cur.execute(f"UPDATE {ENV_DB[e]}._source_channel_config SET {sets} WHERE channel=%s",
                                (*values.values(), p["channel"]))
        finally:
            conn.close()
        return Result(output=f"{p['channel']} -> {values}")


@register
class Trickle(Action):
    name = "trickle"

    def _domain_b(self, p, env, cur=None) -> bool:
        """Reviews (Domain B) land in dev always. In prod they are withheld until the drive team delivers them -
        and once PRD has received reviews (E1's Q2 step, S4's resolution, S3) they keep arriving with every batch,
        instead of going stale. `domain_b:` in the YAML overrides both."""
        if "domain_b" in p:
            return bool(p["domain_b"])
        if env == "dev":
            return True
        if cur is None:   # render / dry-run: decided at run time
            return False
        cur.execute("SELECT COUNT(*) FROM analytics_prod.raw_olist_order_reviews")
        return cur.fetchone()[0] > 0

    def render(self, p, ctx):
        require(p, "env", "days")
        out = []
        for e in envs(p["env"]):
            line = f"CALL {ENV_DB[e]}.sp_trickle_olist_batch({int(p['days'])}, {int(self._domain_b(p, e))});"
            if e == "prod" and "domain_b" not in p:
                line += "   -- reviews: 0 until PRD has received Domain B, then 1"
            out.append(line)
        return out

    def run(self, p, ctx):
        out = []
        conn = connect(ctx)
        try:
            with conn.cursor() as cur:
                for e in envs(p["env"]):
                    db = ENV_DB[e]
                    domain_b = self._domain_b(p, e, cur)
                    cur.execute(f"USE {db}")
                    cur.execute("CALL sp_trickle_olist_batch(%s, %s)", (int(p["days"]), int(domain_b)))
                    cur.execute("SELECT current_watermark, batch_seq FROM _trickle_state LIMIT 1")
                    wm, seq = cur.fetchone()
                    out.append(f"{db}: batch #{seq} watermark {wm}" + ("" if domain_b else " (reviews withheld)"))
        finally:
            conn.close()
        return Result(output="; ".join(out))


@register
class ResetChannels(Action):
    name = "reset_channels"

    def render(self, p, ctx):
        return [f"CALL {ENV_DB[e]}.sp_reset_channel_config();" for e in envs(p.get("env", "both"))]

    def run(self, p, ctx):
        conn = connect(ctx)
        try:
            with conn.cursor() as cur:
                for e in envs(p.get("env", "both")):
                    cur.execute(f"USE {ENV_DB[e]}")   # procedure bodies resolve tables against the session database
                    cur.execute("CALL sp_reset_channel_config()")
        finally:
            conn.close()
        return Result(output="all channels healthy")


@register
class ResetLanding(Action):
    name = "reset_landing"

    def render(self, p, ctx):
        days = int(p.get("days", 90))
        return [f"CALL {ENV_DB[e]}.sp_reset_olist_landing({days}, {int(e == 'dev')});" for e in envs(p.get("env", "both"))]

    def run(self, p, ctx):
        days = int(p.get("days", 90))
        conn = connect(ctx)
        try:
            with conn.cursor() as cur:
                for e in envs(p.get("env", "both")):
                    cur.execute(f"USE {ENV_DB[e]}")
                    cur.execute("CALL sp_reset_olist_landing(%s, %s)", (days, int(e == "dev")))
        finally:
            conn.close()
        return Result(output=f"landing reset to {days} days")
