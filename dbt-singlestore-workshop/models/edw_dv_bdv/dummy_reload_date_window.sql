{{
    config(
        materialized="incremental",
        unique_key="id",
        meta={"reload_strategy": {"type": "date_window", "column": "event_date"}},
    )
}}

with
    generated_dates as (
        -- Genereert dagelijkse rijen van 2023-01-01 tot 2026-01-01
        select
            cast(date_add('2023-01-01', interval seq day) as datetime) as event_date,
            seq :> integer as id,
            concat('payload_', seq) :> varchar(64) as val
        from {{ ref("ref_dummy_numbers") }}
        where date_add('2023-01-01', interval seq day) <= '2026-01-01'
    )
select *
from generated_dates
