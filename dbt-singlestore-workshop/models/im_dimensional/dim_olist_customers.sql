{{
    config(
        materialized="table",
        primary_key=["customer_hkey"],
        shard_key=["customer_hkey"],
    )
}}

with
    customers as (
        select
            customer_hkey,
            customer_id,
            customer_unique_id,
            customer_zip_code_prefix,
            customer_city,
            customer_state,
            _ingested_at
        from {{ ref("stg_olist_customers") }}
    )

select
    customer_hkey,
    customer_id,
    customer_unique_id,
    customer_zip_code_prefix,
    customer_city,
    customer_state,
    _ingested_at
from customers
