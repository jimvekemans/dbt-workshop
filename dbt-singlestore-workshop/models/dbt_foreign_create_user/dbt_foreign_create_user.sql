with
    dbt_models as (
        select name as table_name, database_name as table_schema
        from {{ source("vdp_mgmt", "dbt_run_results") }}
        where resource_type = 'model' and run_started_at >= current_date()
    ),
    dwh_info as (select * from {{ ref("singlestore_table_info") }}),
    final as (
        select di.*
        from dwh_info di
        join dbt_models dm
        where di.table_name = dm.table_name and create_user <> "{{ target.user }}"
    )
select *
from final
