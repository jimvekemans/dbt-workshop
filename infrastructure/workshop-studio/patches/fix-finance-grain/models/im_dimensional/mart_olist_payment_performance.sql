{{
    config(
        materialized='table'
    )
}}

-- Finance data product: revenue per order month. Payments are aggregated to ORDER grain before joining the fact.
with per_order as (
    select
        order_hkey,
        sum(payment_value) as betaald_totaal,
        count(*) as aantal_betalingen
    from {{ ref('stg_olist_payments') }}
    group by order_hkey
)

select
    f.bestel_jaar_maand,
    count(distinct f.order_hkey) as aantal_orders,
    sum(p.betaald_totaal) as omzet,
    round(avg(p.betaald_totaal), 2) as gem_orderwaarde,
    sum(case when p.aantal_betalingen > 1 then 1 else 0 end) as orders_met_meerdere_betalingen
from {{ ref('fct_olist_orders') }} f
left join per_order p on p.order_hkey = f.order_hkey
group by f.bestel_jaar_maand
