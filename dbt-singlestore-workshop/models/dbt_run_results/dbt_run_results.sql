with
    final as (
        select
            null :> varchar(250) as invocation_id,
            null :> datetime(6) as run_started_at,
            null :> varchar(1000) as unique_id,
            null :> varchar(1000) as file_path,
            null :> varchar(250) as relation_name,
            null :> varchar(250) as database_name,
            null :> varchar(1000) as name,
            null :> varchar(250) as resource_type,
            null :> varchar(250) as materialized,
            null :> varchar(250) as status,
            null :> varchar(1000) as failures,
            null :> datetime(6) as started_at,
            null :> datetime(6) as completed_at,
            null :> decimal(38, 10) as execution_time,
            null :> bigint as rows_affected,
            null :> longtext as pre_hook,
            null :> longtext as post_hook,
            null :> longtext as compiled_code
    )
select *
from final
where 1 = 0
