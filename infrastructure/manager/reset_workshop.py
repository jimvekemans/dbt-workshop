#!/usr/bin/env python3
"""
Workshop Reset & Olist Trickle Controller CLI.

Examples:
  # Reset everything (SingleStore landing tables, GitLab branches/tags, LDV & PRD deployments) to clean start state:
  python3 reset_workshop.py

  # Advance Dev (analytics_dev) watermark by 14 days (loads orders, customers, Domain A payments, Domain B reviews):
  python3 reset_workshop.py --trickle-dev 14

  # Advance Prod (analytics_prod) watermark by 14 days (Domain B reviews withheld by default):
  python3 reset_workshop.py --trickle-prod 14

  # Simulate late delivery of Domain B reviews in Prod (analytics_prod) for a subsequent release (e.g. release/2026-Q2):
  python3 reset_workshop.py --trickle-prod 14 --enable-domain-b

  # Workshop Studio fault injection (per-channel config, same knobs as the studio UI):
  python3 reset_workshop.py --channel kafka --delay-batches 2 --env prod     # payments arrive 2 batches late
  python3 reset_workshop.py --channel drive --duplicate-pct 100 --env prod   # reviews file delivered twice
  python3 reset_workshop.py --channel webapp --schema-variant v2             # new order_status 'fraud_hold'
  python3 reset_workshop.py --channel carrier --late-pct 50 --env dev        # generated feed: late carrier scans
  python3 reset_workshop.py --channel web --bot-events 30000 --env dev       # generated feed: crawler traffic
  python3 reset_workshop.py --reset-channels                                 # all channels healthy again
  python3 reset_workshop.py --show-channels                                  # print config + delivery log

  # SingleStore milestones (restore points of the whole warehouse, manager/milestones.sh):
  python3 reset_workshop.py --milestone list
  python3 reset_workshop.py --milestone create before-demo
  python3 reset_workshop.py --milestone restore baseline                     # what the studio Reset does

Runs in the manager container: infrastructure/workshop.sh reset [args] (the examples above are its arguments).
"""

import argparse
import os
import subprocess
import sys

try:
    import pymysql
except ImportError:
    sys.exit("Missing deps (PyMySQL, requests): run it in the manager container - infrastructure/workshop.sh reset ... "
             "(or install infrastructure/requirements.txt into a venv off the exFAT drive)")

from bootstrap_stack import (
    MILESTONES_SCRIPT,
    ROOT_PAT,
    S2_HOST,
    S2_PORT,
    S2_ROOT_PWD,
    apply_studio_schema,
    create_baseline_milestone,
    ensure_ci_job_token_signing_key,
    initialize_workshop_deployments,
    reset_channel_config,
    seed_gitlab_api,
    seed_singlestore,
)


def trickle_env(env_db: str, days_step: int, enable_domain_b: bool):
    conn = pymysql.connect(
        host=S2_HOST,
        port=S2_PORT,
        user="root",
        password=S2_ROOT_PWD,
        database=env_db,
        autocommit=True,
    )
    try:
        with conn.cursor() as cur:
            cur.execute("SELECT current_watermark, batch_seq FROM _trickle_state LIMIT 1;")
            old_wm, old_seq = cur.fetchone()
            cur.execute(
                "CALL sp_trickle_olist_batch(%s, %s);",
                (int(days_step), 1 if enable_domain_b else 0),
            )
            cur.execute("SELECT current_watermark, batch_seq FROM _trickle_state LIMIT 1;")
            new_wm, new_seq = cur.fetchone()
            cur.execute(
                "SELECT "
                "(SELECT count(*) FROM raw_olist_orders), "
                "(SELECT count(*) FROM raw_olist_order_payments), "
                "(SELECT count(*) FROM raw_olist_order_reviews);"
            )
            o_cnt, p_cnt, r_cnt = cur.fetchone()
            print(
                f"[{env_db}] Trickled +{days_step}d ({old_wm} -> {new_wm}, batch #{new_seq}, "
                f"domain_b={enable_domain_b}) | orders={o_cnt:,} | payments(A)={p_cnt:,} | reviews(B)={r_cnt:,}"
            )
    finally:
        conn.close()


CHANNEL_FIELDS = {
    "enabled": ("enabled", int),
    "delay_batches": ("delay_batches", int),
    "duplicate_pct": ("duplicate_pct", int),
    "partial_pct": ("partial_pct", int),
    "null_key_pct": ("null_key_pct", int),
    "schema_variant": ("schema_variant", str),
    "late_pct": ("late_pct", int),
    "bot_events": ("bot_events", int),
}

# Airflow Variables the Workshop Studio uses as levers between deploy windows (airflow/utils/dbt_runtime_overrides.py)
LEVER_VARIABLES = ("disabled_dbt_models", "dbt_daily_load_dbtignore", "dbt_env_vars")


def clear_lever_variables():
    """A clean start has none of the levers set: delete them in both Airflows (absent / Airflow down is fine)."""
    for env in ("dev", "prod"):
        for key in LEVER_VARIABLES:
            subprocess.run(["docker", "exec", f"airflow-{env}", "airflow", "variables", "delete", key],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    print(f"  -> Airflow lever Variables cleared in airflow-dev and airflow-prod: {', '.join(LEVER_VARIABLES)}")


def _env_dbs(env: str):
    return {"dev": ["analytics_dev"], "prod": ["analytics_prod"], "both": ["analytics_dev", "analytics_prod"]}[env]


def _connect(db=None):
    return pymysql.connect(host=S2_HOST, port=S2_PORT, user="root", password=S2_ROOT_PWD, database=db, autocommit=True)


def set_channel(env: str, channel: str, values: dict):
    """Update _source_channel_config for a channel; the next trickle behaves accordingly."""
    conn = _connect()
    try:
        with conn.cursor() as cur:
            for db in _env_dbs(env):
                sets = ", ".join(f"{col}=%s" for col in values) + ", updated_at=NOW()"
                cur.execute(f"UPDATE {db}._source_channel_config SET {sets} WHERE channel=%s", (*values.values(), channel))
                print(f"[{db}] channel {channel}: {values}")
    finally:
        conn.close()


def show_channels():
    conn = _connect()
    try:
        with conn.cursor() as cur:
            for db in _env_dbs("both"):
                cur.execute(
                    f"SELECT channel, enabled, delay_batches, duplicate_pct, partial_pct, null_key_pct, schema_variant, "
                    f"late_pct, bot_events FROM {db}._source_channel_config ORDER BY channel"
                )
                print(f"[{db}] channel config (channel, enabled, delay, dup%, partial%, nullkey%, schema, late%, bots):")
                for row in cur.fetchall():
                    print("   ", row)
                cur.execute(
                    f"SELECT batch_seq, channel, rows_landed, rows_held, faults FROM {db}._delivery_log "
                    f"ORDER BY batch_seq DESC, channel LIMIT 9"
                )
                print(f"[{db}] last deliveries (batch, channel, landed, held, faults):")
                for row in cur.fetchall():
                    print("   ", row)
    finally:
        conn.close()


def reset_channels():
    conn = _connect()
    try:
        with conn.cursor() as cur:
            apply_studio_schema(cur)
            reset_channel_config(cur)
        print("Channel config reset: webapp, kafka, drive, carrier, web healthy in dev + prod.")
    finally:
        conn.close()


def main():
    parser = argparse.ArgumentParser(description="Reset the VDAB dbt workshop or trickle Olist data.")
    parser.add_argument(
        "--initial-days",
        type=int,
        default=90,
        help="Initial number of Olist days when resetting to baseline (default: 90).",
    )
    parser.add_argument(
        "--trickle-dev",
        type=int,
        default=None,
        help="Advance analytics_dev watermark by N days without resetting.",
    )
    parser.add_argument(
        "--trickle-prod",
        type=int,
        default=None,
        help="Advance analytics_prod watermark by N days without resetting.",
    )
    parser.add_argument(
        "--enable-domain-b",
        action="store_true",
        help="When trickling analytics_prod, enable loading Domain B (raw_olist_order_reviews).",
    )
    parser.add_argument("--channel", choices=["webapp", "kafka", "drive", "carrier", "web"],
                        help="Source channel to configure (webapp=orders+customers, kafka=payments/Domain A, drive=reviews/Domain B, "
                             "carrier=tracking scans, web=clickstream).")
    parser.add_argument("--env", choices=["dev", "prod", "both"], default="both", help="Landing env for --channel (default: both).")
    parser.add_argument("--enabled", type=int, choices=[0, 1], help="Channel delivers at all (0/1).")
    parser.add_argument("--delay-batches", type=int, help="Hold each batch back N trickles (late delivery).")
    parser.add_argument("--duplicate-pct", type=int, help="Re-send X%% of each batch (duplicate file).")
    parser.add_argument("--partial-pct", type=int, help="Only X%% of each batch lands (partial file).")
    parser.add_argument("--null-key-pct", type=int, help="X%% of rows arrive with a NULL business key.")
    parser.add_argument("--schema-variant", choices=["v1", "v2"],
                        help="Source contract version (webapp v2: order_status 'fraud_hold'; carrier v2: API v2 payloads; "
                             "web v2: customer_ref = web account).")
    parser.add_argument("--late-pct", type=int, help="carrier: X%% of the pick-up/in-transit scans arrive 2 batches late.")
    parser.add_argument("--bot-events", type=int, help="web: a crawler adds N page views per batch (one cookie).")
    parser.add_argument("--reset-channels", action="store_true", help="(Re)apply the studio schema and make all channels healthy.")
    parser.add_argument("--show-channels", action="store_true", help="Print channel config and the latest delivery log.")
    parser.add_argument("--milestone", nargs="+", metavar=("ACTION", "NAME"),
                        help="SingleStore milestones: list | create NAME | restore NAME | delete NAME "
                             "(snapshots of the stopped data volume; SingleStore restarts).")
    args = parser.parse_args()

    if args.milestone:
        action, *rest = args.milestone
        if action not in ("list", "create", "restore", "delete") or (action != "list" and len(rest) != 1):
            parser.error("--milestone list | create NAME | restore NAME | delete NAME")
        sys.exit(subprocess.run(["bash", MILESTONES_SCRIPT, action, *rest]).returncode)

    if args.reset_channels:
        reset_channels()
        return
    if args.show_channels:
        show_channels()
        return
    if args.channel:
        values = {col: cast(getattr(args, name)) for name, (col, cast) in CHANNEL_FIELDS.items() if getattr(args, name) is not None}
        if not values:
            parser.error("--channel needs at least one of --enabled/--delay-batches/--duplicate-pct/--partial-pct/--null-key-pct/"
                         "--schema-variant/--late-pct/--bot-events")
        set_channel(args.env, args.channel, values)
        return

    if args.trickle_dev is not None or args.trickle_prod is not None:
        if args.trickle_dev is not None:
            trickle_env("analytics_dev", args.trickle_dev, enable_domain_b=True)
        if args.trickle_prod is not None:
            trickle_env("analytics_prod", args.trickle_prod, enable_domain_b=args.enable_domain_b)
        return

    print("=== Resetting VDAB Enterprise dbt Workshop to Clean Start State ===")
    seed_singlestore(force_reload_archive=False, initial_trickle_days=args.initial_days)
    ensure_ci_job_token_signing_key()
    seed_gitlab_api(ROOT_PAT, register_runner=False)   # re-pushes the local dbt project: CI/selector fixes reach GitLab
    initialize_workshop_deployments()
    clear_lever_variables()
    create_baseline_milestone()   # the studio Reset restores this
    print("=== Workshop Reset Complete! ===")


if __name__ == "__main__":
    main()
