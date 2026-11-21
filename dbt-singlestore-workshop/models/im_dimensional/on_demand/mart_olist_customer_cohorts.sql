{{ config(materialized="table", tags=["on-demand"]) }}

-- Heavy report, built on demand (tag on-demand: the selectors keep it out of the nightly
-- load). Repeat-purchase cohorts: customers are grouped by the month of their first
-- order. For every month after that we count how many of them ordered again and how long
-- those deliveries took. Build it with: dbt build --select tag:on-demand
with
    orders as (
        select customer_unique_id, order_purchase_timestamp, levertermijn_dagen
        from {{ ref("fct_olist_orders") }}
        where customer_unique_id is not null and order_purchase_timestamp is not null
    ),

    first_orders as (
        select customer_unique_id, min(order_purchase_timestamp) as first_order_ts
        from orders
        group by customer_unique_id
    ),

    cohort_orders as (
        select
            o.customer_unique_id,
            date_format(f.first_order_ts, '%Y-%m') as cohort_maand,
            timestampdiff(
                month, f.first_order_ts, o.order_purchase_timestamp
            ) as maanden_sinds_eerste_order,
            o.levertermijn_dagen
        from orders o
        inner join first_orders f on f.customer_unique_id = o.customer_unique_id
    ),

    cohort_sizes as (
        select cohort_maand, count(distinct customer_unique_id) as cohort_klanten
        from cohort_orders
        where maanden_sinds_eerste_order = 0
        group by cohort_maand
    ),

    per_month as (
        select
            cohort_maand,
            maanden_sinds_eerste_order,
            count(distinct customer_unique_id) as aantal_klanten,
            count(*) as aantal_orders,
            round(avg(levertermijn_dagen), 1) as gem_levertermijn_dagen
        from cohort_orders
        group by cohort_maand, maanden_sinds_eerste_order
    )

select
    p.cohort_maand,
    p.maanden_sinds_eerste_order,
    s.cohort_klanten,
    p.aantal_klanten,
    p.aantal_orders,
    round(100 * p.aantal_klanten / nullif(s.cohort_klanten, 0), 1) as pct_van_cohort,
    p.gem_levertermijn_dagen
from per_month p
inner join cohort_sizes s on s.cohort_maand = p.cohort_maand
