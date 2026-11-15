-- Fixture model: code blocks in the tests are cut out of this file.
with old_format as (
    select event_id, event_ts
    from {{ source('carrier', 'raw_events') }}
    where event_ts < '{{ var("cut_over") }}'
),

new_format as (
    select event_id, event_ts
    from {{ source('carrier', 'raw_events_v2') }}
    where event_ts >= '{{ var("cut_over") }}'
)

select * from old_format
union all
select * from new_format
