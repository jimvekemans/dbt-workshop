{{ config(severity="warn") }}

-- Makes unreadable scans visible without stopping the load (warn only): one row per
-- scan that could not be read with the format chosen for its arrival moment.
select event_id, read_as, _ingested_at
from {{ ref("stg_carrier_events") }}
where not is_readable
