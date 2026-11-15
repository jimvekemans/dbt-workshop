-- Tests that fail or warn in their newest run of today, with their failure count.
-- A test whose newest run today passed or was skipped is not listed.
with
    ranked as (
        select
            file_path,
            name,
            status,
            failures,
            run_started_at,
            execution_time,
            row_number() over (
                partition by unique_id order by run_started_at desc
            ) as run_rank
        from {{ source("vdp_mgmt", "dbt_run_results") }}
        where resource_type = 'test' and run_started_at >= current_date()
    )
select
    file_path,
    name,
    status,
    case when failures regexp '^[0-9]+$' then failures :> int(4) end as failures,
    run_started_at,
    execution_time :> decimal(10, 2) as execution_time_seconds
from ranked
where run_rank = 1 and status in ('fail', 'error', 'warn')
order by failures desc, name
