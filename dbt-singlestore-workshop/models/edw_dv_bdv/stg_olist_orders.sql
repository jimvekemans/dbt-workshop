{#- val/prd variant of stg_olist_orders. The dev variant, with the malformed-customer-id filter, is
    models/edw_dv_bdv/dev_only/stg_olist_orders.sql - see there for the model-swapping pattern. -#}
{{
    config(
        enabled=env_var("DBT_DEPLOY_TARGET", "dev") != "dev",
        materialized="incremental",
        unique_key="order_id",
        shard_key=["order_id"],
        sort_key=["_ingested_at"],
        on_schema_change="sync_all_columns",
    )
}}

with
    raw_source as (
        select *
        from {{ source("raw_olist", "raw_olist_orders") }}
        {% if is_incremental() %}
            where
                _ingested_at
                > (select coalesce(max(_ingested_at), '1970-01-01') from {{ this }})
        {% endif %}
    ),

    deduped as (
        select
            md5(order_id) as order_hkey,
            order_id,
            md5(customer_id) as customer_hkey,
            customer_id,
            coalesce(
                cast(date_format(order_purchase_timestamp, '%Y%m%d') as signed), -1
            ) as x_bestel_datum_dakey,
            order_status,
            order_purchase_timestamp,
            order_approved_at,
            order_delivered_carrier_date,
            order_delivered_customer_date,
            order_estimated_delivery_date,
            _ingested_at,
            row_number() over (partition by order_id order by _ingested_at desc) as rn
        from raw_source
        where order_id is not null
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
