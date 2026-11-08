"""IFW Olist Trickle Feeder DAG.

Simulates enterprise IFW / Kafka landing zone ingestion into SingleStore (`analytics_dev` or `analytics_prod`):
- Advances the virtual simulation watermark (`_trickle_state.current_watermark`) by `days_step` days.
- Inserts newly purchased orders and simulates CDC updates (`processing` -> `shipped` -> `delivered`).
- Loads corresponding customers and Domain A (`raw_olist_order_payments`) records.
- Conditionally loads Domain B (`raw_olist_order_reviews`) records based on `enable_domain_b_reviews`
  (enabled by default in LDV/dev; withheld by default in PRD/prod until Domain B delivers its source feed).
- Emits Airflow Dataset outlet `singlestore://olist/raw_olist_orders` to trigger `dbt_daily_load`.
"""

import logging
import os
from datetime import datetime

import pymysql
from utils.compat import DAG, Dataset, Param, ParamsDict, get_variable, task

logger = logging.getLogger(__name__)

OLIST_RAW_DATASET = Dataset("singlestore://olist/raw_olist_orders")

DEPLOY_TARGET = os.getenv("DBT_DEPLOY_TARGET", "LDV").upper()
DEFAULT_LANDING_DB = "analytics_prod" if DEPLOY_TARGET in ("PRD", "CBT") else "analytics_dev"
DEFAULT_ENABLE_DOMAIN_B = DEPLOY_TARGET not in ("PRD", "CBT")


with DAG(
    dag_id="ifw_olist_trickle",
    start_date=datetime(2026, 1, 1),
    schedule=None,
    catchup=False,
    max_active_runs=1,
    tags=["ifw", "olist", "trickle", "landing"],
    doc_md="""
## IFW Olist Trickle Feeder (`ifw_olist_trickle`)

Advances the Olist simulation watermark in SingleStore and trickles a new slice of orders, CDC status updates, customers, Domain A payments, and (optionally) Domain B customer reviews into the active landing database (`analytics_dev` in Dev, `analytics_prod` in Prod).

### Parameters & Airflow Variables
- **`days_step`**: Number of simulation days to advance (default: `7`, or Airflow Variable `OLIST_TRICKLE_DAYS_STEP`).
- **`enable_domain_b_reviews`**: Whether Domain B (`raw_olist_order_reviews`) is delivered in this environment. Defaults to `True` in Dev (`analytics_dev`) and `False` in Prod (`analytics_prod`), or Airflow Variable `ENABLE_DOMAIN_B_REVIEWS`.
- **`reset_landing`**: If `True`, resets the landing tables back to the clean 90-day baseline (`2017-03-01` + 90 days) before completing.
""",
    params=ParamsDict(
        {
            "days_step": Param(
                7,
                type="integer",
                description="Number of days to advance the Olist simulation clock.",
            ),
            "enable_domain_b_reviews": Param(
                DEFAULT_ENABLE_DOMAIN_B,
                type="boolean",
                description="Load Domain B (raw_olist_order_reviews) during this trickle batch.",
            ),
            "reset_landing": Param(
                False,
                type="boolean",
                description="Reset landing tables back to the initial 90-day workshop baseline instead of advancing.",
            ),
        }
    ),
) as dag:

    @task(
        task_id="trickle_olist_slice",
        outlets=[OLIST_RAW_DATASET],
    )
    def trickle_olist_slice(**context):
        params = context.get("params", {})
        days_step = int(
            params.get(
                "days_step",
                get_variable("OLIST_TRICKLE_DAYS_STEP", "7"),
            )
        )

        var_domain_b = get_variable("ENABLE_DOMAIN_B_REVIEWS", None)
        if var_domain_b is not None:
            enable_domain_b = str(var_domain_b).strip().lower() in ("true", "1", "yes")
        else:
            enable_domain_b = bool(
                params.get("enable_domain_b_reviews", DEFAULT_ENABLE_DOMAIN_B)
            )

        reset_landing = bool(params.get("reset_landing", False))

        s2_host = os.getenv("SINGLESTORE_HOST", "singlestore")
        s2_port = int(os.getenv("SINGLESTORE_PORT", "3306"))
        s2_user = os.getenv("SINGLESTORE_USER", "root")
        s2_pass = os.getenv("SINGLESTORE_PASS", "WorkshopRootPwd123!")

        conn = pymysql.connect(
            host=s2_host,
            port=s2_port,
            user=s2_user,
            password=s2_pass,
            database=DEFAULT_LANDING_DB,
            autocommit=True,
        )
        try:
            with conn.cursor() as cur:
                cur.execute("SELECT current_watermark, batch_seq FROM _trickle_state LIMIT 1;")
                before_wm, before_seq = cur.fetchone()

                if reset_landing:
                    logger.info(
                        f"[{DEFAULT_LANDING_DB}] Resetting Olist landing tables to 90-day baseline "
                        f"(enable_domain_b={enable_domain_b})..."
                    )
                    cur.execute(
                        "CALL sp_reset_olist_landing(%s, %s);",
                        (90, 1 if enable_domain_b else 0),
                    )
                else:
                    logger.info(
                        f"[{DEFAULT_LANDING_DB}] Advancing Olist watermark from {before_wm} "
                        f"by {days_step} days (enable_domain_b={enable_domain_b})..."
                    )
                    cur.execute(
                        "CALL sp_trickle_olist_batch(%s, %s);",
                        (days_step, 1 if enable_domain_b else 0),
                    )

                cur.execute("SELECT current_watermark, batch_seq FROM _trickle_state LIMIT 1;")
                after_wm, after_seq = cur.fetchone()

                counts = {}
                for tbl in (
                    "raw_olist_customers",
                    "raw_olist_orders",
                    "raw_olist_order_payments",
                    "raw_olist_order_reviews",
                ):
                    cur.execute(f"SELECT count(*) FROM {tbl};")
                    counts[tbl] = cur.fetchone()[0]

                summary = {
                    "landing_db": DEFAULT_LANDING_DB,
                    "previous_watermark": str(before_wm),
                    "current_watermark": str(after_wm),
                    "batch_seq": int(after_seq),
                    "enable_domain_b_reviews": enable_domain_b,
                    "table_counts": counts,
                }
                logger.info(f"Olist trickle batch complete: {summary}")
                return summary
        finally:
            conn.close()

    trickle_olist_slice()
