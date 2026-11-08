with
    run_results as (
        select
            file_path,
            name,
            status,
            failures,
            avg(execution_time) :> int(4) as execution_time_seconds
        from {{ source("vdp_mgmt", "dbt_run_results") }}
        where
            resource_type = 'test'
            and status not in ('pass', 'success')
            and run_started_at >= current_date()
    ),
    final as (select * from run_results group by file_path, name, status, failures)
select *
from final
order by execution_time_seconds desc, failures desc
