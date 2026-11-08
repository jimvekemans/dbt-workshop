"""Live probes: observed backend state -> node health/metrics. Each probe fails independently (the status pills in the
header show which backends are reachable); in Live mode the observed health overrides the simulated one."""
from __future__ import annotations

import time
from typing import Any, Callable

from .actions.airflow import list_runs, read_variables, run_outcome
from .actions.base import Ctx
from .actions.gitlab import GitLab
from .actions.sqlops import connect
from .config import Settings
from .schema import HEALTH_RANK, World

CHANNELS = ("webapp", "kafka", "drive", "carrier", "web")
LANDING = {   # landing node -> (channel, table)
    "landing.raw_olist_orders": ("webapp", "raw_olist_orders"),
    "landing.raw_olist_customers": ("webapp", "raw_olist_customers"),
    "landing.raw_olist_order_payments": ("kafka", "raw_olist_order_payments"),
    "landing.raw_olist_order_reviews": ("drive", "raw_olist_order_reviews"),
    "landing.raw_carrier_tracking_events": ("carrier", "raw_carrier_tracking_events"),
    "landing.raw_web_events": ("web", "raw_web_events"),
}
NEWEST_SQL = {   # newest *business* timestamp that landed (same definition as macros/utils/olist_freshness.sql)
    "raw_olist_orders": "(SELECT MAX(order_purchase_timestamp) FROM {db}.raw_olist_orders)",
    "raw_olist_customers": "(SELECT MAX(order_purchase_timestamp) FROM {db}.raw_olist_orders)",
    "raw_olist_order_payments": "(SELECT MAX(o.order_purchase_timestamp) FROM {db}.raw_olist_orders o "
                                "INNER JOIN {db}.raw_olist_order_payments p ON p.order_id = o.order_id)",
    "raw_olist_order_reviews": "(SELECT MAX(review_creation_date) FROM {db}.raw_olist_order_reviews)",
    # carrier API v1 (local time) or v2 (UTC ISO-8601), like macros/utils/carrier_payload.sql
    "raw_carrier_tracking_events": "(SELECT MAX(COALESCE(CAST(JSON_EXTRACT_STRING(payload, 'event_time') AS DATETIME), "
                                   "DATE_SUB(CAST(REPLACE(REPLACE(JSON_EXTRACT_STRING(payload, 'eventTime'), 'T', ' '), "
                                   "'Z', '') AS DATETIME), INTERVAL 3 HOUR))) FROM {db}.raw_carrier_tracking_events)",
    "raw_web_events": "(SELECT MAX(event_ts) FROM {db}.raw_web_events WHERE visitor_id NOT LIKE 'bot-%')",
}
RUN_STATE = {"success": "ok", "failed": "fail", "running": "warn", "queued": "warn"}


def worst(a: str, b: str) -> str:
    return a if HEALTH_RANK[a] >= HEALTH_RANK[b] else b


def probe_singlestore(ctx: Ctx, world_ids: set[str]) -> dict[str, dict]:
    out: dict[str, dict] = {}
    conn = connect(ctx)
    try:
        with conn.cursor() as cur:
            for env, db in (("prod", "analytics_prod"), ("dev", "analytics_dev")):
                cur.execute(f"SELECT current_watermark, batch_seq FROM {db}._trickle_state LIMIT 1")
                wm, seq = cur.fetchone() or (None, 0)
                cur.execute(f"SELECT channel, enabled, delay_batches, duplicate_pct, partial_pct, null_key_pct, schema_variant, "
                            f"late_pct, bot_events FROM {db}._source_channel_config")
                cfg = {r[0]: r[1:] for r in cur.fetchall()}
                cur.execute(f"SELECT channel, COUNT(*) FROM {db}._held_batches GROUP BY channel")
                held = dict(cur.fetchall())
                cur.execute(f"SELECT d.channel, d.rows_landed, d.rows_held, d.faults FROM {db}._delivery_log d "
                            f"WHERE d.batch_seq = (SELECT MAX(batch_seq) FROM {db}._delivery_log)")
                last = {r[0]: r[1:] for r in cur.fetchall()}
                for ch in CHANNELS:
                    # a source team's DEV and PRD feeds are separate: the channel shows the worse of the two
                    nid = f"channel.{ch}"
                    c = cfg.get(ch)
                    if not c or nid not in world_ids:
                        continue
                    enabled, delay, dup, partial, nullkey, variant, late, bots = c
                    faulty = (delay > 0 or dup > 0 or partial < 100 or nullkey > 0 or variant != "v1" or late > 0 or bots > 0
                              or held.get(ch, 0) > 0)
                    health = "fail" if not enabled else "warn" if faulty else "ok"
                    if ch == "drive" and health == "ok" and last.get("drive", (0, 0, ""))[2] == "not_delivered;":
                        health = "idle"
                    note = ""
                    if faulty or not enabled:
                        parts = [] if enabled else ["disabled"]
                        parts += [f"delay={delay} batches (held {held.get(ch, 0)})"] if delay or held.get(ch, 0) else []
                        parts += [f"dup={dup}%"] if dup else []
                        parts += [f"partial={partial}%"] if partial < 100 else []
                        parts += [f"nullkeys={nullkey}%"] if nullkey else []
                        parts += [f"schema={variant}"] if variant != "v1" else []
                        parts += [f"late scans={late}%"] if late else []
                        parts += [f"bot hits={bots}/batch"] if bots else []
                        note = f"{'PRD' if env == 'prod' else 'LDV'}: " + ", ".join(parts)
                    landed, h_rows, faults = last.get(ch, (0, 0, ""))
                    entry = {"health": health, "note": note}
                    if env == "prod":
                        entry["metrics"] = {"rows_landed_last": landed, "rows_held_last": h_rows, "faults": faults or "-",
                                            "batch": seq}
                    prev = out.get(nid)
                    if prev is None:
                        out[nid] = {"metrics": {}, **entry}
                    else:
                        prev["metrics"].update(entry.get("metrics", {}))
                        if HEALTH_RANK[entry["health"]] > HEALTH_RANK[prev["health"]]:
                            prev["health"], prev["note"] = entry["health"], entry["note"]
                        elif entry["note"] and prev["health"] == entry["health"] and entry["health"] != "ok":
                            prev["note"] = "; ".join(x for x in (prev["note"], entry["note"]) if x)
                for nid, (ch, table) in LANDING.items():
                    if nid not in world_ids:
                        continue
                    cur.execute(f"SELECT COUNT(*) FROM {db}.{table}")
                    rows = cur.fetchone()[0]
                    key = "rows" if env == "prod" else "rows_dev"
                    out.setdefault(nid, {"health": "ok", "note": "", "metrics": {}})["metrics"][key] = rows
                    if env != "prod":
                        continue
                    cur.execute(f"SELECT TIMESTAMPDIFF(DAY, {NEWEST_SQL[table].format(db=db)}, "
                                f"(SELECT current_watermark FROM {db}._trickle_state LIMIT 1))")
                    lag = cur.fetchone()[0]
                    entry = out[nid]
                    entry["metrics"]["lag_days"] = lag if lag is not None else "n/a"
                    if rows == 0:
                        entry["health"], entry["note"] = ("idle", "withheld in PRD until delivered") if ch == "drive" else ("fail", "no data")
                    elif lag is not None and lag > 3:
                        entry["health"], entry["note"] = "fail", f"freshness {lag}d > 3d"
                    elif lag is not None and lag > 1:
                        entry["health"], entry["note"] = "warn", f"freshness {lag}d > 1d"
                if env == "prod" and "db.prod" in world_ids:
                    out["db.prod"] = {"health": "ok", "metrics": {"watermark": str(wm), "batch": seq}}
                if env == "dev" and "db.dev" in world_ids:
                    out["db.dev"] = {"health": "ok", "metrics": {"watermark": str(wm), "batch": seq}}
    finally:
        conn.close()
    return out


def evaluate_tests(latest: list[tuple], tests_by_node: dict[str, list[str]],
                   models_by_node: dict[str, list[str]] | None = None) -> dict[str, dict]:
    """Node health from the latest result per test (name pattern) and per model (exact name).

    `latest` rows are (name, status) or (name, status, run_started_at[, resource_type]). Per node only the results of
    the NEWEST run that touched it count: a test that newer runs no longer execute (the v1 model after a version
    switch, a test that moved) must not keep the node red forever. A node is only reported when at least one of its
    tests or models actually has a result: no evidence means no observation (the map then shows the simulated health,
    and a step check on that node says it cannot be verified live) - never a made-up 'ok'."""
    models_by_node = models_by_node or {}
    rows = [(r[0], r[1], r[2] if len(r) > 2 else "", r[3] if len(r) > 3 else "test") for r in latest]
    out: dict[str, dict] = {}
    for nid in set(tests_by_node) | set(models_by_node):
        patterns, models = tests_by_node.get(nid, []), models_by_node.get(nid, [])
        matched = [(name, status, ts) for name, status, ts, rtype in rows
                   if (rtype != "model" and any(p in name for p in patterns)) or (rtype == "model" and name in models)]
        if not matched:
            continue
        newest = max(str(ts) for _, _, ts in matched)
        matched = [(name, status) for name, status, ts in matched if str(ts) == newest]
        failing = [name for name, status in matched if status in ("fail", "error")]
        warning = [name for name, status in matched if status in ("warn", "skipped")]
        health = "fail" if failing else "warn" if warning else "ok"
        note = ("failing: " + ", ".join(failing[:2])) if failing else ("warning: " + ", ".join(warning[:2])) if warning else ""
        out[nid] = {"health": health, "note": note, "metrics": {"tests": len(matched), "failing_tests": len(failing)}}
    return out


def probe_dbt_results(ctx: Ctx, tests_by_node: dict[str, list[str]], since: str | None = None,
                      models_by_node: dict[str, list[str]] | None = None) -> dict[str, dict]:
    """Latest result per test (and per model) from vdp_mgmt.dbt_run_results (filled when LOG_RUN_RESULTS=TRUE).

    LDV and PRD build into the same model databases in this stack (target `run`, SINGLESTORE_DB), so the latest
    result of a test - whichever environment ran it - is the state of the table it checks. Results from before the
    last live Reset (`since`) are ignored: a test that only existed in an abandoned scenario must not stay red."""
    if not tests_by_node and not models_by_node:
        return {}
    fence = "AND run_started_at >= %s" if since else ""
    args = (since, since) if since else ()
    conn = connect(ctx)
    try:
        with conn.cursor() as cur:
            cur.execute(f"""
                SELECT r.name, r.status, r.run_started_at, r.resource_type FROM vdp_mgmt.dbt_run_results r
                JOIN (SELECT name, MAX(run_started_at) AS ts FROM vdp_mgmt.dbt_run_results
                      WHERE resource_type IN ('test', 'unit_test', 'model') {fence} GROUP BY name) l
                  ON l.name = r.name AND l.ts = r.run_started_at
                WHERE r.resource_type IN ('test', 'unit_test', 'model') {fence.replace('run_started_at', 'r.run_started_at')}""",
                        args)
            latest = cur.fetchall()
    finally:
        conn.close()
    return evaluate_tests(list(latest), tests_by_node, models_by_node)


def probe_gitlab(ctx: Ctx, world_ids: set[str]) -> dict[str, dict]:
    gl = GitLab(ctx)
    out: dict[str, dict] = {}
    branches = [b["name"] for b in gl.req("GET", gl.p("/repository/branches"), params={"per_page": 100}).json()]
    tags = [t["name"] for t in gl.req("GET", gl.p("/repository/tags"), params={"per_page": 100, "order_by": "updated"}).json()]
    pipes = gl.req("GET", gl.p("/pipelines"), params={"per_page": 10, "order_by": "id", "sort": "desc"}).json()
    feats = [b for b in branches if b.startswith(("feature/", "bugfix/"))]
    rel = sorted(b for b in branches if b.startswith("release/"))
    dev_tags = [t for t in tags if t.startswith("v")]
    main_tags = [t for t in tags if t[:2] == "20"]
    if "git.feature" in world_ids:
        out["git.feature"] = {"health": "ok" if feats else "idle", "metrics": {"branches": ", ".join(feats) or "-"}}
    if "git.dev" in world_ids:
        out["git.dev"] = {"health": "ok", "metrics": {"latest_tag": dev_tags[0] if dev_tags else "-"}}
    if "git.release" in world_ids:
        out["git.release"] = {"health": "ok" if rel else "idle", "metrics": {"branches": ", ".join(rel) or "-"}}
    if "git.main" in world_ids:
        out["git.main"] = {"health": "ok", "metrics": {"latest_tag": main_tags[0] if main_tags else "-"}}
    real = [x for x in pipes if x["status"] not in ("skipped", "canceled")]   # e.g. the changelog branch pipeline is skipped
    if "ci.verify" in world_ids and real:
        p = real[0]
        out["ci.verify"] = {"health": {"success": "ok", "manual": "ok", "failed": "fail"}.get(p["status"], "warn"),
                            "note": f"{p['ref']}: {p['status']}", "metrics": {"pipeline": p["id"], "ref": p["ref"]}}
    return out


def latest_run(runs: list[dict[str, Any]], since: str | None = None) -> dict[str, Any] | None:
    """Newest run that started after the last live Reset (queued runs have no start_date yet: they are new)."""
    def started(r: dict[str, Any]) -> str:
        return (r.get("start_date") or "").replace("T", " ")[:19]

    fresh = [r for r in runs if not since or not r.get("start_date") or started(r) >= since]
    if not fresh:
        return None
    pending = [r for r in fresh if not r.get("start_date")]
    return pending[0] if pending else max(fresh, key=started)


def probe_airflow(ctx: Ctx, world_ids: set[str], since: str | None = None) -> tuple[dict[str, dict], dict[str, Any]]:
    """dbt_daily_load: the newest run in EITHER environment (LDV and PRD build the same tables in this stack, so the
    newest load is the state of the map); dbt_release: PRD. airflow.levers: the lever Variables set in each Airflow."""
    out: dict[str, dict] = {}
    status: dict[str, Any] = {}
    loads: list[tuple[str, dict[str, Any]]] = []
    levers: dict[str, dict[str, str]] = {}
    for env in ("dev", "prod"):
        try:
            name = f"{ctx.settings.airflow_container_prefix}{env}"
            alive = ctx.sh(["docker", "inspect", "-f", "{{.State.Running}}", name], check=False, timeout=15)
            if alive.returncode != 0 or alive.stdout.strip() != "true":
                status[f"airflow-{env}"] = {"ok": False, "error": f"container {name} is not running (or docker socket unavailable)"}
                continue
            if "airflow.daily_load" in world_ids:
                latest = latest_run(list_runs(ctx, env, "dbt_daily_load"), since)
                if latest:
                    loads.append((env, latest))
            if "airflow.release" in world_ids and env == "prod":
                latest = latest_run(list_runs(ctx, env, "dbt_release"), since)
                if latest:
                    state, note = run_outcome(ctx, env, "dbt_release", latest)
                    out["airflow.release"] = {"health": RUN_STATE.get(state, "idle"), "note": note,
                                              "metrics": {"last_run": latest.get("run_id", "")[-19:], "state": state}}
            if "airflow.levers" in world_ids:
                values = read_variables(ctx, env)
                if values is not None:
                    levers[env] = {k: v for k, v in values.items() if v not in ("", "[]", "{}")}
            status[f"airflow-{env}"] = {"ok": True}
        except Exception as e:  # noqa: BLE001
            status[f"airflow-{env}"] = {"ok": False, "error": str(e)[:120]}
    if loads:
        # queued runs have no start_date yet: they are the newest
        env, latest = max(loads, key=lambda el: (el[1].get("start_date") is None, (el[1].get("start_date") or "")))
        # run_outcome: dbt_daily_load ends "success" even when dbt_build failed - the key task decides
        state, note = run_outcome(ctx, env, "dbt_daily_load", latest)
        label = "LDV" if env == "dev" else "PRD"
        out["airflow.daily_load"] = {"health": RUN_STATE.get(state, "idle"), "note": f"{label}: {note or state}",
                                     "metrics": {"env": label, "last_run": latest.get("run_id", "")[-19:], "state": state}}
    if levers:
        active = [f"{'LDV' if env == 'dev' else 'PRD'} {k}={v}" for env, vals in sorted(levers.items()) for k, v in vals.items()]
        out["airflow.levers"] = {"health": "warn" if active else "ok",
                                 "note": "; ".join(active)[:240] if active else "no lever set",
                                 "metrics": {f"{'LDV' if env == 'dev' else 'PRD'}": len(vals) for env, vals in levers.items()}}
    return out, status


def propagate(world: World, observed: dict[str, dict]) -> dict[str, dict]:
    """A failing/warning upstream makes its (non-failing) consumers warn - the ripple you see when zooming out."""
    adj: dict[str, list[str]] = {}
    for e in world.edges:
        if e.kind == "data":
            adj.setdefault(e.source, []).append(e.target)
    labels = {n.id: n.label for n in world.nodes}
    changed = True
    while changed:
        changed = False
        for src, targets in adj.items():
            sh = observed.get(src, {}).get("health")
            if sh not in ("fail", "warn"):
                continue
            for t in targets:
                cur = observed.get(t, {}).get("health", "ok")
                if cur in ("idle", "ok"):
                    observed.setdefault(t, {"metrics": {}})
                    observed[t].update(health="warn", note=observed[t].get("note") or f"upstream: {labels[src]}")
                    changed = True
    return observed


def run_probes(settings: Settings, world: World, cache: dict[str, Any],
               since: str | None = None) -> tuple[dict[str, dict], dict[str, Any]]:
    ctx = Ctx(settings=settings, dry_run=False, cache=cache)
    ids = {n.id for n in world.nodes}
    tests_by_node = {n.id: n.tests for n in world.nodes if n.tests}
    models_by_node = {n.id: n.models for n in world.nodes if n.models}
    observed: dict[str, dict] = {}
    status: dict[str, Any] = {"checked": time.strftime("%H:%M:%S")}

    def guarded(name: str, fn: Callable[[], dict[str, dict]]) -> None:
        try:
            observed_part = fn()
            for nid, v in observed_part.items():
                cur = observed.setdefault(nid, {"metrics": {}})
                cur["metrics"] = {**cur.get("metrics", {}), **v.get("metrics", {})}
                if v.get("health"):
                    if cur.get("health") and v["health"] != cur["health"]:
                        v = {**v, "health": worst(v["health"], cur["health"])}
                    cur["health"] = v["health"]
                if v.get("note"):
                    cur["note"] = v["note"]
            status[name] = {"ok": True}
        except Exception as e:  # noqa: BLE001 - unreachable backend is a state, not a crash
            status[name] = {"ok": False, "error": str(e)[:140]}

    guarded("singlestore", lambda: probe_singlestore(ctx, ids))
    guarded("dbt", lambda: probe_dbt_results(ctx, tests_by_node, since, models_by_node))
    guarded("gitlab", lambda: probe_gitlab(ctx, ids))
    af, af_status = probe_airflow(ctx, ids, since)
    for nid, v in af.items():
        observed[nid] = v
    status.update(af_status)
    return propagate(world, observed), status
