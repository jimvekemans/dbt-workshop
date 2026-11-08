{{
    config(
        materialized='table'
    )
}}

-- Refactor: reuse dim_olist_payments instead of re-aggregating the staging model.
select
    f.bestel_jaar_maand,
    count(distinct f.order_hkey) as aantal_orders,
    sum(p.order_betaald_totaal) as omzet,
    round(avg(p.order_betaald_totaal), 2) as gem_orderwaarde,
    sum(case when p.payment_sequential > 1 then 1 else 0 end) as orders_met_meerdere_betalingen
from {{ ref('fct_olist_orders') }} f
left join {{ ref('dim_olist_payments') }} p on p.order_hkey = f.order_hkey
group by f.bestel_jaar_maand
