{{
    config(
        materialized='table',
        primary_key=['order_hkey'],
        shard_key=['order_hkey']
    )
}}

with orders as (
    select *
    from {{ ref('stg_olist_orders') }}
),

customers as (
    select
        customer_hkey,
        customer_unique_id,
        customer_city,
        customer_state
    from {{ ref('dim_olist_customers') }}
),

kalender as (
    select
        x_datum_dakey,
        datum_dt,
        jaar_maand_lbl6 as bestel_jaar_maand,
        jaar_kwartaal_lbl1 as bestel_jaar_kwartaal
    from {{ ref('datum') }}
),

final as (
    select
        o.order_hkey,
        o.order_id,
        md5(o.order_id) as domain_b_hkey,
        o.customer_hkey,
        c.customer_unique_id,
        c.customer_city,
        c.customer_state,
        o.x_bestel_datum_dakey,
        k.bestel_jaar_maand,
        k.bestel_jaar_kwartaal,
        o.order_status,
        o.order_purchase_timestamp,
        o.order_approved_at,
        o.order_delivered_carrier_date,
        o.order_delivered_customer_date,
        o.order_estimated_delivery_date,
        datediff(o.order_delivered_customer_date, o.order_purchase_timestamp) as levertermijn_dagen,
        o._ingested_at
    from orders o
    left join customers c
        on o.customer_hkey = c.customer_hkey
    left join kalender k
        on o.x_bestel_datum_dakey = k.x_datum_dakey
)

select *
from final
