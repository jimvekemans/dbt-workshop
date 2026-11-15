{{ config(materialized="table", shard_key=["event_id"], sort_key=["_ingested_at"]) }}

-- The carrier switches its scan format on one day. Each scan is read with the format
-- in use when it ARRIVED: before the switch-over moment the old one, after it the new.
-- Operators set the moment without a deploy, through the Airflow Variable dbt_env_vars:
-- {"DBT_CARRIER_V2_FROM": "2017-03-04 12:00:00"}. Default: the new format never starts.
{% set new_format_from = env_var("DBT_CARRIER_V2_FROM", "9999-12-31 00:00:00") %}

with
    old_format as (
        -- flat snake_case keys, local time
        select
            event_id,
            json_extract_string(payload, 'shipment_id') as shipment_id,
            json_extract_string(payload, 'order_ref') as order_id,
            json_extract_string(payload, 'event') as event_type,
            cast(json_extract_string(payload, 'event_time') as datetime) as event_ts,
            'old format' as read_as,
            _ingested_at
        from {{ source("raw_carrier", "raw_carrier_tracking_events") }}
        where _ingested_at < '{{ new_format_from }}'
    ),

    new_format as (
        -- camelCase keys, nested status code, UTC time (-> local time)
        select
            event_id,
            json_extract_string(payload, 'shipmentId') as shipment_id,
            json_extract_string(payload, 'orderReference') as order_id,
            case
                json_extract_string(payload, 'status', 'code')
                when 'PU'
                then 'picked_up'
                when 'IT'
                then 'in_transit'
                when 'DL'
                then 'delivered'
            end as event_type,
            date_sub(
                cast(
                    replace(
                        replace(json_extract_string(payload, 'eventTime'), 'T', ' '),
                        'Z',
                        ''
                    ) as datetime
                ),
                interval 3 hour
            ) as event_ts,
            'new format' as read_as,
            _ingested_at
        from {{ source("raw_carrier", "raw_carrier_tracking_events") }}
        where _ingested_at >= '{{ new_format_from }}'
    ),

    combined as (
        select
            *,
            shipment_id is not null
            and event_type is not null
            and event_ts is not null as is_readable
        from
            (
                select *
                from old_format
                union all
                select *
                from new_format
            ) as both_formats
    )

select *
from combined
