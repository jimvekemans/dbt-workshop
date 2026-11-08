{#-
    DYNAMIC MODEL SWAPPING - the DEV variant of stg_olist_orders.

    Two models are called stg_olist_orders; exactly one is enabled per environment, so refs and the DAG never change:
      models/edw_dv_bdv/stg_olist_orders.sql           PRD / CBT : lean, no DEV-only logic
      models/edw_dv_bdv/dev_only/stg_olist_orders.sql  LDV / ABT : also removes the QA team's smoke-test orders
    Putting the QA filter in the shared model instead would ship an anti-join against a DEV-only table to PRD: extra work
    on every PRD load for rows that never exist there - and a ref to a source PRD does not even have.
    Both variants must keep the same columns: whatever the DEV variant changes, the PRD variant has to mirror.
-#}
{{
    config(
        enabled=env_var('DBT_DEPLOY_TARGET', 'LDV') not in ['PRD', 'CBT'],
        materialized='incremental',
        unique_key='order_id',
        shard_key=['order_id'],
        sort_key=['_ingested_at'],
        on_schema_change='sync_all_columns'
    )
}}

with raw_source as (
    select *
    from {{ source('raw_olist', 'raw_olist_orders') }}
    {% if is_incremental() %}
      where _ingested_at > (select coalesce(max(_ingested_at), '1970-01-01') from {{ this }})
    {% endif %}
),

qa_accounts as (
    select account_id from {{ source('qa', '_qa_test_accounts') }}
),

deduped as (
    select
        md5(r.order_id) as order_hkey,
        r.order_id,
        md5(r.customer_id) as customer_hkey,
        r.customer_id,
        coalesce(cast(date_format(r.order_purchase_timestamp, '%Y%m%d') as signed), -1) as x_bestel_datum_dakey,
        r.order_status,
        r.order_purchase_timestamp,
        r.order_approved_at,
        r.order_delivered_carrier_date,
        r.order_delivered_customer_date,
        r.order_estimated_delivery_date,
        r._ingested_at,
        row_number() over (partition by r.order_id order by r._ingested_at desc) as rn
    from raw_source r
    left join qa_accounts qa
        on qa.account_id = r.customer_id
    where r.order_id is not null
      and qa.account_id is null     -- DEV only: QA smoke-test orders
)

select
    order_hkey,
    order_id,
    customer_hkey,
    customer_id,
    x_bestel_datum_dakey,
    order_status,
    order_purchase_timestamp,
    order_approved_at,
    order_delivered_carrier_date,
    order_delivered_customer_date,
    order_estimated_delivery_date,
    _ingested_at
from deduped
where rn = 1
