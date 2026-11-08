{{
    config(
        materialized="incremental",
        unique_key="entry_hash",
        incremental_strategy="delete+insert",
        database="edw_dv_bdv",
        enabled=target.name == "reload",
    )
}}

with
    batch as (
        select
            '{{ get_incr_load_start() }}' as window_start,
            '{{ get_incr_load_end() }}' as window_end,
            '{{ get_local_run_started_at() }}' as run_started_at
    ),
    final as (
        select
            b.*,
            '{{ flags.FULL_REFRESH }}' as full_refresh_flag,
            md5(concat(b.window_start, b.window_end, b.run_started_at)) as entry_hash
    )
select *
from final
