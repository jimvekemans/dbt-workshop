{{
    config(
        materialized='table',
        shard_key=['event_id'],
        sort_key=['_ingested_at']
    )
}}

-- Staging is the contract boundary: explicit columns, renamed to OUR names. When the web-analytics team renames,
-- adds or drops a column again, this model fails loudly at the boundary - owned and watched by the platform team -
-- instead of the change flowing silently into models nobody watches. Their `traffic_source` stays `referrer` for
-- everything downstream.
select
    event_id,
    visitor_id,
    customer_ref,
    event_type,
    page,
    traffic_source as referrer,
    event_ts,
    _ingested_at
from {{ source('raw_web', 'raw_web_events') }}
where event_id is not null
