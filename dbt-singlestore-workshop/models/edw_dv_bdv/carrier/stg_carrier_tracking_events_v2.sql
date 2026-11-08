{{
    config(
        materialized='table',
        shard_key=['event_id'],
        sort_key=['_ingested_at']
    )
}}

-- Version 2: parses BOTH carrier API versions - v1 (flat, local time) and v2 (camelCase, nested status code, UTC).
-- Shipped to PRD ahead of the carrier's switch and enabled with an environment variable, without a deploy:
--   DBT_VERSION_STG_CARRIER_TRACKING_EVENTS=2   (Airflow Variable dbt_env_vars, see carrier.yml)
with raw_source as (
    select event_id, payload, _ingested_at
    from {{ source('raw_carrier', 'raw_carrier_tracking_events') }}
)

select
    event_id,
    coalesce(json_extract_string(payload, 'shipmentId'), json_extract_string(payload, 'shipment_id')) as shipment_id,
    coalesce(json_extract_string(payload, 'orderReference'), json_extract_string(payload, 'order_ref')) as order_id,
    case json_extract_string(payload, 'status', 'code')
        when 'PU' then 'picked_up'
        when 'IT' then 'in_transit'
        when 'DL' then 'delivered'
        else json_extract_string(payload, 'event')
    end as event_type,
    {{ carrier_scan_ts('payload') }} as event_ts,   -- v2 UTC -> local business time
    if(json_extract_string(payload, 'eventTime') is not null, 'v2', 'v1') as api_version,
    _ingested_at
from raw_source
