{{
    config(
        materialized='table',
        shard_key=['event_id'],
        sort_key=['_ingested_at']
    )
}}

-- Version 1: parses carrier API v1 only (flat payload, local time). A v2 payload yields NULL keys and timestamps -
-- the critical not_null / accepted_values tests in carrier.yml stop it there.
with raw_source as (
    select event_id, payload, _ingested_at
    from {{ source('raw_carrier', 'raw_carrier_tracking_events') }}
)

select
    event_id,
    json_extract_string(payload, 'shipment_id') as shipment_id,
    json_extract_string(payload, 'order_ref') as order_id,
    json_extract_string(payload, 'event') as event_type,
    cast(json_extract_string(payload, 'event_time') as datetime) as event_ts,
    'v1' as api_version,
    _ingested_at
from raw_source
