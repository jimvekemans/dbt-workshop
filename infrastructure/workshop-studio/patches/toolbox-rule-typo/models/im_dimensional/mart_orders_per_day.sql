{{ config(materialized="table") }}

select
    str_to_date(cast(order_purchase_timestamp as char), '%Y-%m-%d') as order_day,
    count(*) as orders
from {{ ref("fct_olist_orders") }}
group by 1
