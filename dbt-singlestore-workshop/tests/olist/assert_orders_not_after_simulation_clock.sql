{{ config(
    severity=env_var('DBT_SEVERITY_DEV_DATA', 'warn' if env_var('DBT_DEPLOY_TARGET', 'LDV') in ['LDV', 'ABT'] else 'error'),
    tags=['critical']
) }}

-- No order can be purchased after the business clock (the simulation watermark). In PRD that is a hard error: the
-- source sends timestamps from the future. In DEV it happens every time the QA smoke tests run - they stamp their test
-- orders with the wall clock - so the same rule only warns there (dynamic local severity, see sources_olist.yml).
{%- set env_db = 'analytics_prod' if env_var('DBT_DEPLOY_TARGET', 'LDV') in ['PRD', 'CBT'] else 'analytics_dev' %}
select o.order_id, o.customer_id, o.order_purchase_timestamp, w.current_watermark
from {{ source('raw_olist', 'raw_olist_orders') }} o
cross join (select current_watermark from {{ env_db }}._trickle_state limit 1) w
where o.order_purchase_timestamp > w.current_watermark
