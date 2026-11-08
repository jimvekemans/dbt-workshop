{{
    config(
        materialized='table'
    )
}}

-- Marketing: web conversion per month. A checkout is attributed to the order that the same PERSON placed at the moment
-- of the checkout (+/- 10 minutes).
-- The web team's customer_ref has two definitions over time: the webshop customer_id (issued per order, contract v1)
-- and the web account = the person (contract v2, the webapp team's customer_unique_id). Both resolve to
-- customer_unique_id here, and the join uses that - the one key both teams agree on.
with events as (
    select
        event_id,
        visitor_id,
        customer_ref,
        event_type,
        event_ts,
        date_format(event_ts, '%Y%m') as maand
    from {{ ref('stg_web_events') }}
    where visitor_id is not null
),

checkouts as (
    select
        e.*,
        coalesce(c.customer_unique_id, e.customer_ref) as customer_unique_id   -- v1: look up the person; v2: is the person
    from events e
    left join {{ ref('dim_olist_customers') }} c
        on c.customer_id = e.customer_ref
    where e.event_type = 'checkout'
      and e.customer_ref is not null
),

attributed as (
    select
        c.maand,
        c.event_id,
        o.order_id
    from checkouts c
    left join {{ ref('fct_olist_orders') }} o
        on o.customer_unique_id = c.customer_unique_id
       and o.order_purchase_timestamp between date_sub(c.event_ts, interval 10 minute)
                                          and date_add(c.event_ts, interval 10 minute)
),

visitors as (
    select maand, count(distinct visitor_id) as bezoekers
    from events
    group by maand
),

conversions as (
    -- distinct: a person with two orders inside the window must not count a checkout twice
    select maand, count(distinct event_id) as checkouts, count(distinct order_id) as toegewezen_orders
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
left join conversions c
    on c.maand = v.maand
