{{ config(materialized="table", primary_key=["order_hkey"], shard_key=["order_hkey"]) }}

-- One row per shipped order: pick-up, delivery and the CURRENT status of the parcel.
-- Scans come from stg_carrier_events: each one is read with the format that was in use
-- when it arrived (switch-over moment DBT_CARRIER_V2_FROM). Unreadable scans are left
-- out.
with
    scans as (
        select *
        from {{ ref("stg_carrier_events") }}
        where is_readable and order_id is not null
    ),

    latest_scan as (
        -- Current status = the most recently RECEIVED scan (newest _ingested_at).
        -- Right as long as scans arrive in the
        -- order they happen; a scan uploaded late overrules a delivery that already
        -- arrived.
        select
            order_id,
            event_type,
            event_ts,
            row_number() over (
                partition by order_id order by _ingested_at desc, event_ts desc
            ) as rn
        from scans
    )

select
    md5(s.order_id) as order_hkey,
    s.order_id,
    max(s.shipment_id) as shipment_id,
    min(case when s.event_type = 'picked_up' then s.event_ts end) as picked_up_at,
    max(case when s.event_type = 'delivered' then s.event_ts end) as delivered_at,
    l.event_type as current_status,
    l.event_ts as current_status_ts,
    count(*) as scans
from scans s
inner join latest_scan l on l.order_id = s.order_id and l.rn = 1
group by s.order_id, l.event_type, l.event_ts
