"""SQL regression + fault-injection tests for singlestore/studio_schema.sql.

Needs a SingleStore with _olist_master loaded and the studio schema applied (e.g. a throwaway
singlestoredb-dev container seeded with bootstrap_stack.seed_singlestore()). Skipped unless
STUDIO_TEST_S2_PORT is set:

    STUDIO_TEST_S2_PORT=13306 STUDIO_TEST_S2_PWD='TestRoot123!' pytest tests/test_sql_trickle.py
"""
import os
import pathlib
import sys

import pytest

pymysql = pytest.importorskip("pymysql")

PORT = os.environ.get("STUDIO_TEST_S2_PORT")
pytestmark = pytest.mark.skipif(not PORT, reason="STUDIO_TEST_S2_PORT not set (no SingleStore test instance)")

ROOT = pathlib.Path(__file__).resolve().parents[3] / "manager"   # bootstrap_stack.py lives in infrastructure/manager/
sys.path.insert(0, str(ROOT))

RAW = {
    "raw_olist_orders": "order_id,customer_id,order_status,order_purchase_timestamp,order_approved_at,"
                        "order_delivered_carrier_date,order_delivered_customer_date,order_estimated_delivery_date",
    "raw_olist_customers": "customer_id,customer_unique_id,customer_zip_code_prefix,customer_city,customer_state",
    "raw_olist_order_payments": "order_id,payment_sequential,payment_type,payment_installments,payment_value",
    "raw_olist_order_reviews": "review_id,order_id,review_score,review_comment_title,review_comment_message,"
                               "review_creation_date,review_answer_timestamp",
}
DDL_COPY = list(RAW)


def connect(db=None):
    return pymysql.connect(host="127.0.0.1", port=int(PORT), user="root",
                           password=os.environ.get("STUDIO_TEST_S2_PWD", "TestRoot123!"),
                           database=db, autocommit=True)


def run_script(cur, text):
    from bootstrap_stack import parse_sql_script
    for stmt in parse_sql_script(text):
        cur.execute(stmt)


def fingerprint(cur, db):
    out = {}
    for t, cols in RAW.items():
        concat = ",'|',".join(f"IFNULL(CAST({c} AS CHAR),'~')" for c in cols.split(","))
        cur.execute(f"SELECT COUNT(*), IFNULL(SUM(CRC32(CONCAT({concat}))),0) FROM {db}.{t}")
        out[t] = tuple(int(x) for x in cur.fetchone())
    return out


@pytest.fixture()
def env():
    conn = connect()
    cur = conn.cursor()
    cur.execute("CREATE DATABASE IF NOT EXISTS studio_legacy")
    cur.execute("USE studio_legacy")
    for t in RAW:
        cur.execute(f"CREATE TABLE IF NOT EXISTS studio_legacy.{t} LIKE analytics_dev.{t}")
        cur.execute(f"TRUNCATE TABLE studio_legacy.{t}")
    cur.execute("CREATE TABLE IF NOT EXISTS studio_legacy._trickle_state LIKE analytics_dev._trickle_state")
    cur.execute("DELETE FROM studio_legacy._trickle_state")
    cur.execute("INSERT INTO studio_legacy._trickle_state VALUES ('dev','2017-03-01 00:00:00',0,NOW())")
    run_script(cur, (pathlib.Path(__file__).parent / "legacy_trickle.sql").read_text())
    cur.execute("USE analytics_dev")
    cur.execute("CALL sp_reset_channel_config()")
    cur.execute("CALL sp_reset_olist_landing(0, TRUE)")
    yield cur
    cur.execute("CALL sp_reset_channel_config()")
    cur.execute("CALL sp_reset_olist_landing(0, TRUE)")
    conn.close()


def legacy(cur, days, domain_b):
    """Procedure bodies resolve unqualified tables against the session DB, so switch to the legacy DB."""
    cur.execute("USE studio_legacy")
    cur.execute("CALL sp_trickle_olist_batch(%s,%s)", (days, domain_b))
    cur.execute("USE analytics_dev")


def set_channel(cur, channel, **kw):
    sets = ", ".join(f"{k}=%s" for k in kw)
    cur.execute(f"UPDATE analytics_dev._source_channel_config SET {sets} WHERE channel=%s", (*kw.values(), channel))


def count(cur, table):
    cur.execute(f"SELECT COUNT(*) FROM analytics_dev.{table}")
    return cur.fetchone()[0]


def test_default_config_is_identical_to_legacy_procedure(env):
    cur = env
    for days, domain_b in [(90, 1), (14, 1), (14, 0), (14, 1), (7, 0)]:
        legacy(cur, days, domain_b)
        cur.execute("CALL analytics_dev.sp_trickle_olist_batch(%s,%s)", (days, domain_b))
        assert fingerprint(cur, "analytics_dev") == fingerprint(cur, "studio_legacy")
    cur.execute("SELECT current_watermark, batch_seq FROM analytics_dev._trickle_state")
    new_state = cur.fetchone()
    cur.execute("SELECT current_watermark, batch_seq FROM studio_legacy._trickle_state")
    assert new_state == cur.fetchone()


def test_delivery_log_is_written_per_channel(env):
    cur = env
    cur.execute("CALL sp_trickle_olist_batch(14, 1)")
    cur.execute("SELECT channel, rows_landed, rows_held, faults FROM _delivery_log ORDER BY channel")
    rows = cur.fetchall()
    assert [r[0] for r in rows] == ["carrier", "drive", "kafka", "web", "webapp"]
    assert all(r[1] > 0 and r[2] == 0 and r[3] == "" for r in rows)


def test_every_batch_gets_a_later_ingest_time(env):
    """Batches that run within the same second must still be distinguishable: arrival order matters (late scans)."""
    cur = env
    for _ in range(3):
        cur.execute("CALL sp_trickle_olist_batch(1, 1)")
    cur.execute("SELECT COUNT(DISTINCT _ingested_at) FROM raw_olist_orders")
    assert cur.fetchone()[0] == 3


def test_carrier_api_v2_payloads(env):
    cur = env
    cur.execute("CALL sp_trickle_olist_batch(30, 1)")
    cur.execute("SELECT COUNT(*) FROM raw_carrier_tracking_events WHERE JSON_EXTRACT_STRING(payload, 'event_time') IS NOT NULL")
    assert cur.fetchone()[0] > 0
    set_channel(cur, "carrier", schema_variant="v2")
    cur.execute("CALL sp_trickle_olist_batch(7, 1)")
    cur.execute("SELECT payload FROM raw_carrier_tracking_events WHERE JSON_EXTRACT_STRING(payload, 'eventTime') IS NOT NULL LIMIT 1")
    import json
    v2 = json.loads(cur.fetchone()[0])
    assert set(v2) == {"shipmentId", "orderReference", "status", "eventTime", "apiVersion"}
    assert v2["status"]["code"] in ("PU", "IT", "DL") and v2["eventTime"].endswith("Z")


def test_carrier_late_scans_arrive_after_their_delivery(env):
    cur = env
    cur.execute("CALL sp_trickle_olist_batch(60, 1)")
    set_channel(cur, "carrier", late_pct=50)
    for _ in range(6):
        cur.execute("CALL sp_trickle_olist_batch(1, 1)")
    cur.execute("SELECT COUNT(*) FROM _held_carrier_events")
    assert cur.fetchone()[0] > 0                                   # the last two batches' late scans are still out
    # some parcel's in-transit / pick-up scan landed AFTER its delivery scan
    cur.execute("""SELECT COUNT(*) FROM raw_carrier_tracking_events d JOIN raw_carrier_tracking_events l
                   ON JSON_EXTRACT_STRING(l.payload, 'order_ref') = JSON_EXTRACT_STRING(d.payload, 'order_ref')
                  AND JSON_EXTRACT_STRING(d.payload, 'event') = 'delivered'
                  AND JSON_EXTRACT_STRING(l.payload, 'event') <> 'delivered' AND l._ingested_at > d._ingested_at""")
    assert cur.fetchone()[0] > 0
    cur.execute("SELECT faults FROM _delivery_log WHERE channel='carrier' ORDER BY batch_seq DESC LIMIT 1")
    assert "late_scans" in cur.fetchone()[0]


def test_web_customer_ref_definitions_and_crawler(env):
    cur = env
    cur.execute("CALL sp_trickle_olist_batch(7, 1)")
    cur.execute("""SELECT COUNT(*) FROM raw_web_events w JOIN raw_olist_customers c ON c.customer_id = w.customer_ref
                   WHERE w.event_type = 'checkout'""")
    v1_matches = cur.fetchone()[0]
    assert v1_matches > 0                                            # v1: the webshop's per-order customer_id
    set_channel(cur, "web", schema_variant="v2", bot_events=5000)
    cur.execute("CALL sp_trickle_olist_batch(7, 1)")
    cur.execute("""SELECT COUNT(*) FROM raw_web_events w JOIN raw_olist_customers c ON c.customer_unique_id = w.customer_ref
                   WHERE w.event_type = 'checkout' AND w._ingested_at = (SELECT MAX(_ingested_at) FROM raw_web_events)""")
    assert cur.fetchone()[0] > 0                                     # v2: the person
    cur.execute("SELECT COUNT(*), COUNT(DISTINCT page) FROM raw_web_events WHERE visitor_id = 'bot-crawler-7'")
    assert cur.fetchone() == (5000, 5000)


def test_web_loader_follows_a_renamed_column_and_reset_restores_it(env):
    cur = env
    cur.execute("ALTER TABLE raw_web_events CHANGE referrer traffic_source")
    cur.execute("CALL sp_trickle_olist_batch(7, 1)")
    cur.execute("SELECT COUNT(*), COUNT(traffic_source) FROM raw_web_events")
    total, filled = cur.fetchone()
    assert total > 0 and total == filled
    cur.execute("CALL sp_reset_olist_landing(0, TRUE)")
    cur.execute("SELECT COUNT(*) FROM information_schema.columns WHERE table_schema='analytics_dev' "
                "AND table_name='raw_web_events' AND column_name='referrer'")
    assert cur.fetchone()[0] == 1


def test_qa_smoke_test_is_dev_only(env):
    cur = env
    cur.execute("CALL sp_qa_smoke_test(25)")
    cur.execute("SELECT COUNT(*), MIN(order_purchase_timestamp) > '2020-01-01' FROM raw_olist_orders WHERE customer_id LIKE 'qa-%'")
    assert cur.fetchone() == (25, 1)
    cur.execute("CALL sp_qa_cleanup()")
    assert count(cur, "raw_olist_orders") == 0
    cur.execute("SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='analytics_prod' AND table_name='_qa_test_accounts'")
    assert cur.fetchone()[0] == 0


def test_kafka_delay_holds_then_releases_late(env):
    cur = env
    legacy(cur, 14, 1)
    expected_payments_14d = count_legacy(cur, "raw_olist_order_payments")
    set_channel(cur, "kafka", delay_batches=2)
    cur.execute("CALL sp_trickle_olist_batch(14, 1)")      # batch 1: held
    assert count(cur, "raw_olist_order_payments") == 0
    cur.execute("SELECT COUNT(*) FROM _held_batches WHERE channel='kafka'")
    assert cur.fetchone()[0] == 1
    cur.execute("SELECT rows_held, faults FROM _delivery_log WHERE channel='kafka' AND batch_seq=1")
    held, faults = cur.fetchone()
    assert held == expected_payments_14d and "delayed" in faults
    assert count(cur, "raw_olist_orders") > 0               # other channels unaffected
    set_channel(cur, "kafka", delay_batches=0)
    cur.execute("CALL sp_trickle_olist_batch(0, 1)")        # batch 2: nothing due yet (release at 3)
    assert count(cur, "raw_olist_order_payments") == 0
    cur.execute("CALL sp_trickle_olist_batch(0, 1)")        # batch 3: late arrival lands
    assert count(cur, "raw_olist_order_payments") == expected_payments_14d
    cur.execute("SELECT COUNT(*) FROM _held_batches")
    assert cur.fetchone()[0] == 0
    cur.execute("SELECT faults FROM _delivery_log WHERE channel='kafka' AND batch_seq=3")
    assert "late_release" in cur.fetchone()[0]


def count_legacy(cur, table):
    cur.execute(f"SELECT COUNT(*) FROM studio_legacy.{table}")
    return cur.fetchone()[0]


def test_drive_duplicate_100_doubles_reviews_and_breaks_uniqueness(env):
    cur = env
    legacy(cur, 30, 1)
    base = count_legacy(cur, "raw_olist_order_reviews")
    set_channel(cur, "drive", duplicate_pct=100)
    cur.execute("CALL sp_trickle_olist_batch(30, 1)")
    assert count(cur, "raw_olist_order_reviews") == base * 2
    cur.execute("SELECT COUNT(*) FROM (SELECT review_id FROM raw_olist_order_reviews GROUP BY review_id HAVING COUNT(*)>1) d")
    assert cur.fetchone()[0] > 0


def test_drive_partial_40_lands_a_deterministic_subset(env):
    cur = env
    legacy(cur, 30, 1)
    base = count_legacy(cur, "raw_olist_order_reviews")
    set_channel(cur, "drive", partial_pct=40)
    cur.execute("CALL sp_trickle_olist_batch(30, 1)")
    landed = count(cur, "raw_olist_order_reviews")
    assert 0.3 * base < landed < 0.5 * base
    cur.execute("CALL sp_reset_olist_landing(0, TRUE)")
    cur.execute("CALL sp_trickle_olist_batch(30, 1)")
    assert count(cur, "raw_olist_order_reviews") == landed   # deterministic


def test_webapp_schema_v2_emits_fraud_hold(env):
    cur = env
    set_channel(cur, "webapp", schema_variant="v2")
    cur.execute("CALL sp_trickle_olist_batch(30, 1)")
    cur.execute("SELECT COUNT(*) FROM raw_olist_orders WHERE order_status='fraud_hold'")
    assert cur.fetchone()[0] > 0


def test_null_keys_and_disabled_channel(env):
    cur = env
    set_channel(cur, "kafka", null_key_pct=10)
    set_channel(cur, "drive", enabled=0)
    cur.execute("CALL sp_trickle_olist_batch(30, 1)")
    cur.execute("SELECT COUNT(*) FROM raw_olist_order_payments WHERE order_id IS NULL")
    assert cur.fetchone()[0] > 0
    assert count(cur, "raw_olist_order_reviews") == 0
    cur.execute("SELECT faults FROM _delivery_log WHERE channel='drive'")
    assert "channel_disabled" in cur.fetchone()[0]


def test_reset_channels_action_works_from_a_fresh_connection(env):
    """Regression: the studio's reset_channels used CALL db.proc() without USE (procedures use the session DB)."""
    from app.actions.base import Ctx
    from app.actions.sqlops import ResetChannels
    from app.config import Settings

    set_channel(env, "kafka", delay_batches=3)
    s = Settings()
    s.s2_host, s.s2_port = "127.0.0.1", int(PORT)
    s.s2_password = os.environ.get("STUDIO_TEST_S2_PWD", "TestRoot123!")
    ResetChannels().run({"env": "dev"}, Ctx(settings=s))
    env.execute("SELECT delay_batches FROM analytics_dev._source_channel_config WHERE channel='kafka'")
    assert env.fetchone()[0] == 0
