{{
    config(
        materialized='table',
        shard_key=['order_hkey']
    )
}}

-- Payment grain. NOTE: order_betaald_totaal is an ORDER-level measure repeated on every payment row.
select
    domain_a_hkey,
    order_hkey,
    order_id,
    payment_sequential,
    payment_type,
    payment_installments,
    payment_value,
    sum(payment_value) over (partition by order_id) as order_betaald_totaal
from {{ ref('stg_olist_payments') }}
