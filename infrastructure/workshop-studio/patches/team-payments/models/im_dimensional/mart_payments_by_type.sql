{{ config(materialized="table") }}

-- Payments team: how many payments and how much money per way of paying.
select payment_type, count(*) as payments, sum(payment_value) as total_paid
from {{ source("raw_olist", "raw_olist_order_payments") }}
group by payment_type
