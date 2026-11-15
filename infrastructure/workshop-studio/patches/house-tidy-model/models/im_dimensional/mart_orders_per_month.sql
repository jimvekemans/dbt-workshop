{{ config(materialized="table") }}

select date_format(order_purchase_timestamp, '%Y-%m') as order_month, count(*) as orders
from {{ ref("fct_olist_orders") }}
group by 1
