{#-
    olist_loaded_at(table_name)

    Source freshness for the Olist simulation. The Olist business clock runs in 2017 and `_ingested_at` is wall-clock
    time, so neither can be compared with NOW(). Instead freshness is measured against the *simulation watermark*
    (analytics_*._trickle_state): "how far behind the watermark is the newest business record this channel delivered?".
    The query returns NOW() minus that lag, so dbt's `freshness` thresholds (warn_after / error_after) read as
    "days behind the simulation clock".

      webapp  : newest order_purchase_timestamp that landed
      kafka   : newest purchase of an order that has payments landed (Domain A)
      drive   : newest review_creation_date that landed (Domain B)
      carrier : newest scan time that landed (API v1 local time or API v2 UTC, see carrier_scan_ts)
      web     : newest page view that landed
-#}
{% macro olist_loaded_at(table_name) %}
    {%- set env_db = 'analytics_prod' if env_var('DBT_DEPLOY_TARGET', 'LDV') in ['PRD', 'CBT'] else 'analytics_dev' -%}
    {%- if table_name in ['raw_olist_orders', 'raw_olist_customers'] -%}
        {%- set newest = "(select max(order_purchase_timestamp) from " ~ env_db ~ ".raw_olist_orders)" -%}
    {%- elif table_name == 'raw_olist_order_payments' -%}
        {%- set newest = "(select max(o.order_purchase_timestamp) from " ~ env_db ~ ".raw_olist_orders o inner join " ~ env_db ~ ".raw_olist_order_payments p on p.order_id = o.order_id)" -%}
    {%- elif table_name == 'raw_carrier_tracking_events' -%}
        {%- set newest = "(select max(" ~ carrier_scan_ts('payload') ~ ") from " ~ env_db ~ ".raw_carrier_tracking_events)" -%}
    {%- elif table_name == 'raw_web_events' -%}
        {%- set newest = "(select max(event_ts) from " ~ env_db ~ ".raw_web_events where visitor_id not like 'bot-%')" -%}
    {%- else -%}
        {%- set newest = "(select max(review_creation_date) from " ~ env_db ~ ".raw_olist_order_reviews)" -%}
    {%- endif -%}
    select timestampadd(second, -coalesce(timestampdiff(second, {{ newest }}, (select current_watermark from {{ env_db }}._trickle_state limit 1)), 31536000), now())
{% endmacro %}
