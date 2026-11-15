{{ config(materialized="table") }}

-- Marketing: web conversion per month. A checkout is attributed to the order that the
-- same customer placed at the
-- moment of the checkout (+/- 10 minutes). The web team's customer_ref is hashed like
-- the webapp's customer_id and
-- joined on the fact's customer_hkey.
with
    events as (
        select
            visitor_id,
            customer_ref,
            event_type,
            event_ts,
            date_format(event_ts, '%Y%m') as maand
        from {{ ref("stg_web_events") }}
        where visitor_id is not null
    ),

    checkouts as (
        select * from events where event_type = 'checkout' and customer_ref is not null
    ),

    attributed as (
        select c.maand, c.visitor_id, o.order_id
        from checkouts c
        left join
            {{ ref("fct_olist_orders") }} o
            on o.customer_hkey = md5(c.customer_ref)
            and o.order_purchase_timestamp
            between date_sub(c.event_ts, interval 10 minute) and date_add(
                c.event_ts, interval 10 minute
            )
    ),

    visitors as (
        select maand, count(distinct visitor_id) as bezoekers from events group by maand
    ),

    conversions as (
        select maand, count(*) as checkouts, count(order_id) as toegewezen_orders
        from attributed
        group by maand
    )

select
    v.maand,
    v.bezoekers,
    coalesce(c.checkouts, 0) as checkouts,
    coalesce(c.toegewezen_orders, 0) as toegewezen_orders,
    round(100 * coalesce(c.toegewezen_orders, 0) / v.bezoekers, 2) as conversie_pct,
    round(100 * c.toegewezen_orders / nullif(c.checkouts, 0), 1) as toewijzing_pct
from visitors v
left join conversions c on c.maand = v.maand
