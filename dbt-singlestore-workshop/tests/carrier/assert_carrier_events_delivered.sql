{{ config(severity='error', tags=['critical']) }}

-- Delivery guard on the carrier feed: fails when the newest scan that landed is more than 3 days behind the
-- simulation clock - the carrier stopped delivering. Critical: Management's delivery KPI must not be published on
-- half a picture. (Source freshness reports the same lag, but informationally: it never stops the daily load.)
{%- set env_db = 'analytics_prod' if env_var('DBT_DEPLOY_TARGET', 'LDV') in ['PRD', 'CBT'] else 'analytics_dev' %}
select newest_scan, watermark, timestampdiff(day, newest_scan, watermark) as days_behind
from (
    select
        (select max({{ carrier_scan_ts('payload') }}) from {{ source('raw_carrier', 'raw_carrier_tracking_events') }}) as newest_scan,
        (select current_watermark from {{ env_db }}._trickle_state limit 1) as watermark
) g
where newest_scan is null
   or timestampdiff(day, newest_scan, watermark) > 3
