{{ config(severity="error") }}

-- Every order must appear exactly once in the orders table.
select order_hkey, count(*) as copies
from {{ ref("fct_olist_orders") }}
group by order_hkey
having count(*) > 1
