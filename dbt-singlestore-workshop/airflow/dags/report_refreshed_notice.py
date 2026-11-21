"""report_refreshed_notice: another team's DAG that reacts to one refreshed table.

Scheduled on the asset of the management report table (`<env>_im_dimensional.mart_olist_management_kpi`). The asset
gets a new event when dbt_daily_load refreshes the table AND the Airflow Variable `dbt_emit_asset_events` is true (see
dbt_daily_load). Each run sends ONE short plain-text mail - which table, when, by which version, how many rows - through
the mail path of the failure report (DBT_REPORT_SMTP_HOST, Mailpit in the workshop stack: http://localhost:8025).
No external calls.

The DAG is created paused (is_paused_upon_creation): unpause it to switch the notice on. Started by hand it sends a
test notice that says so.
"""

import logging
from datetime import datetime

from airflow.sdk import DAG, task
from airflow.sdk.exceptions import AirflowException

from utils.dbt_asset_utils import collect_events, get_outlet_path, make_asset, render_refresh_notice
from utils.dbt_config import ENV
from utils.dbt_failure_report import send_text_mail

logger = logging.getLogger(__name__)

REPORT_DATABASE = f"{ENV}_im_dimensional"
REPORT_TABLE = "mart_olist_management_kpi"

with DAG(
    dag_id="report_refreshed_notice",
    start_date=datetime(2024, 1, 1),
    schedule=[make_asset(get_outlet_path(REPORT_DATABASE, REPORT_TABLE))],
    catchup=False,
    max_active_runs=1,
    is_paused_upon_creation=True,
    tags=["notice", "asset_consumer"],
) as dag:

    @task(task_id="send_refreshed_notice")
    def send_refreshed_notice(**context):
        """One mail for this run: the newest refresh event of the report table (several events that came in while the
        DAG was paused or busy are one run, one mail)."""
        events = collect_events(context.get("triggering_asset_events"))
        subject, body = render_refresh_notice(events, ENV, REPORT_DATABASE, REPORT_TABLE)
        if not send_text_mail(subject, body):
            raise AirflowException("The notice could not be sent (see the log above and DBT_REPORT_SMTP_HOST).")

    send_refreshed_notice()
