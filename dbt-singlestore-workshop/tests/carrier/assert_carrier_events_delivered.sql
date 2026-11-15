{{ config(severity="warn", tags=["critical"]) }}

-- Delivery guard on the carrier feed: fails when the newest scan that landed is more
-- than 3 days old - the carrier
-- stopped delivering. Warn only: on empty landing tables there is no scan yet, so the
-- guard cannot pass.
select newest_scan, timestampdiff(day, newest_scan, now()) as days_behind
from
    (
        select max({{ carrier_scan_ts("payload") }}) as newest_scan
        from {{ source("raw_carrier", "raw_carrier_tracking_events") }}
    ) g
where newest_scan is null or timestampdiff(day, newest_scan, now()) > 3
