with
    current_table_stats as (
        select table_name, table_rows from {{ ref("dbt_model_info") }}
    ),
    model_execution_times as (
        select
            sec_to_time(avg(rr.execution_time)) as average_runtime,
            avg(rr.execution_time) as average_runtime_seconds,
            sec_to_time(max(rr.execution_time)) as max_runtime,
            case
                when avg(rr.rows_affected) > 0
                then avg(rr.rows_affected)
                when avg(rr.rows_affected) = 0 and max(rr.materialized) = 'table'
                then max(cts.table_rows)
                else null
            end
            :> int as average_rowcount,
            rr.name
        from {{ source("vdp_mgmt", "dbt_run_results") }} rr
        left join current_table_stats cts on rr.name = cts.table_name
        where
            rr.resource_type = 'model'
            and rr.materialized <> 'view'
            and (
                rr.rows_affected > 1
                or (rr.rows_affected = 0 and rr.materialized = 'table')
            )
            and date_trunc('day', rr.run_started_at)
            > (current_date() - interval 1 month)
        group by rr.name
        order by average_runtime desc
    ),
    categorized_models as (
        select
            case
                when average_runtime >= '00:15:00'
                then "(8) >= 15 min"
                when average_runtime > '00:10:00'
                then "(7) 10 - 15 min"
                when average_runtime > '00:05:00'
                then "(6) 5 - 10 min"
                when average_runtime > '00:02:00'
                then "(5) 2 - 5 min"
                when average_runtime > '00:01:00'
                then "(4) 1+ min"
                when average_runtime > '00:00:30'
                then "(3) 30+ sec"
                when average_runtime > '00:00:10'
                then "(2) 10+ sec"
                else "(1) < 10 sec"
            end as category,
            count(category) as models_in_cat,
            group_concat(distinct name) as model_names
        from model_execution_times
        group by category
        order by category
    ),
    table_creation_spedometer as (
        select
            name,
            average_runtime,
            (average_rowcount * 1.0 / nullif(average_runtime_seconds, 0))
            :> decimal(18, 1) as avg_rows_per_sec
        from model_execution_times
        where average_runtime_seconds > 60 and average_rowcount is not null
        order by avg_rows_per_sec asc
    ),
    global_benchmark as (
        select avg(avg_rows_per_sec) :> int as global_avg_rows_per_sec
        from table_creation_spedometer
    ),
    final as (
        select
            tcs.name,
            tcs.average_runtime,
            case
                when tcs.average_runtime >= '00:15:00'
                then "(8) >= 15 min"
                when tcs.average_runtime > '00:10:00'
                then "(7) 10 - 15 min"
                when tcs.average_runtime > '00:05:00'
                then "(6) 5 - 10 min"
                when tcs.average_runtime > '00:02:00'
                then "(5) 2 - 5 min"
                when tcs.average_runtime > '00:01:00'
                then "(4) 1+ min"
                when tcs.average_runtime > '00:00:30'
                then "(3) 30+ sec"
                when tcs.average_runtime > '00:00:10'
                then "(2) 10+ sec"
                else "(1) < 10 sec"
            end as category,
            tcs.avg_rows_per_sec as model_speed,
            gb.global_avg_rows_per_sec as global_avg_speed,
            round(
                (tcs.avg_rows_per_sec / gb.global_avg_rows_per_sec) * 100, 1
            ) as pct_of_global_speed
        from table_creation_spedometer tcs
        cross join global_benchmark gb
        order by tcs.average_runtime desc
    )
select *
from final
order by category desc, pct_of_global_speed asc
