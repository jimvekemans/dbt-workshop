{{
    config(
        materialized='table',
        shard_key=['event_id'],
        sort_key=['_ingested_at']
    )
}}

-- "Lazy" staging: every landing column is passed straight through with select *. Convenient - whatever the
-- web-analytics team adds, renames or drops in their table flows downstream on the next run, unannounced
-- (scenario T2: a renamed column breaks a model nobody maintains anymore). An incremental model with
-- on_schema_change='sync_all_columns' does the same, and also drops the old column's history.
select *
from {{ source('raw_web', 'raw_web_events') }}
where event_id is not null
