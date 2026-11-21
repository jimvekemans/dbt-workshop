{#- The rows are written by the hooks open_load_cycle / close_load_cycle (macros/utils/loading_history.sql), not by this
    model: it only creates the empty table (the same skeleton pattern as dbt_run_results). The history must survive a
    bare `dbt build --full-refresh`: it is only rebuilt when named in `--vars '{full_refresh_models: [dummy_load_cycles]}'`
    (macros/core/should_full_refresh.sql). -#}
{{
    config(
        materialized="incremental",
        incremental_strategy="append",
        primary_key=["object_name", "load_cycle_id"],
        shard_key=["object_name", "load_cycle_id"],
        meta={"full_refresh_only_when_listed": true},
    )
}}

with
    final as (
        select
            null :> bigint as load_cycle_id,
            null :> varchar(150) as object_name,
            null :> varchar(64) as invocation_id,
            null :> datetime(6) as load_start_date,
            null :> datetime(6) as load_end_date,
            null :> int as success_flag
    )
select *
from final
where 1 = 0
