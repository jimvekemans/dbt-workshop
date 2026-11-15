{{ config(materialized="table") }}

-- Management KPI per order month: order volume and delivery performance (carrier
-- scans vs. promised date).
-- Lives OUTSIDE the carrier folders on purpose: a .dbtignore that drops only
-- models/*/carrier/ leaves this ref dangling
-- and the whole project fails to parse - every dependant of an ignored node must be
-- ignored as well.
with
    orders as (
        select {{ star(ref("fct_olist_orders")) }} from {{ ref("fct_olist_orders") }}
    ),

    deliveries as (
        select order_hkey, picked_up_at, delivered_at
        from {{ ref("fct_olist_deliveries") }}
    )

select
    o.bestel_jaar_maand,
    count(*) as aantal_orders,
    count(d.picked_up_at) as aantal_verzonden,
    count(d.delivered_at) as aantal_geleverd,
    round(
        100 * sum(
            case
                when d.delivered_at <= o.order_estimated_delivery_date then 1 else 0
            end
        )
        / nullif(count(d.delivered_at), 0),
        1
    ) as pct_op_tijd,
    round(
        avg(timestampdiff(hour, d.picked_up_at, d.delivered_at)) / 24, 1
    ) as gem_transit_dagen
from orders o
left join deliveries d on d.order_hkey = o.order_hkey
group by o.bestel_jaar_maand
