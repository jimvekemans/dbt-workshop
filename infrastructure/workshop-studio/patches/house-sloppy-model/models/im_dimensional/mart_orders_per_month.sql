{{ config(materialized="table") }}
SELECT to_char(order_purchase_timestamp,'YYYY-MM') AS order_month,COUNT(*) AS orders
FROM {{ ref("fct_olist_orders") }}
GROUP BY 1;
