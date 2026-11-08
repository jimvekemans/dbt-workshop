{{ config(severity='error', tags=['critical']) }}

-- Finance reconciliation: the revenue mart must equal the raw payments (tolerance 0.5%).
with mart as (
    select sum(omzet) as mart_total from {{ ref('mart_olist_payment_performance') }}
),
raw_payments as (
    select sum(payment_value) as raw_total
    from (
        select order_id, payment_sequential, max(payment_value) as payment_value
        from {{ source('raw_olist', 'raw_olist_order_payments') }}
        group by order_id, payment_sequential
    ) d
)

select mart.mart_total, raw_payments.raw_total
from mart
cross join raw_payments
where abs(mart.mart_total - raw_payments.raw_total) > 0.005 * raw_payments.raw_total
