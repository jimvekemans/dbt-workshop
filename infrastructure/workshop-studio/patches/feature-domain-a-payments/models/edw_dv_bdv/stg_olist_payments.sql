{{
    config(
        materialized='table',
        shard_key=['order_id']
    )
}}

-- Domain A (Kafka team). Grain: one row per payment (order_id, payment_sequential).
select
    md5(order_id) as domain_a_hkey,
    md5(order_id) as order_hkey,
    order_id,
    payment_sequential,
    payment_type,
    payment_installments,
    payment_value,
    _ingested_at
from {{ source('raw_olist', 'raw_olist_order_payments') }}
where order_id is not null
