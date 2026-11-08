with
    dbt_models as (
        select concat(database_name, ".", name) as identifier
        from {{ source("vdp_mgmt", "dbt_run_results") }}
        where resource_type = 'model' and run_started_at >= current_date()
    ),
    dwh_info as (
        select distinct table_schema, table_name, table_type, create_user
        from {{ ref("singlestore_table_info") }}
        where table_schema <> "vdp_mgmt"  -- mgmt views might be disabled in project
    ),
    final as (
        select *
        from dwh_info
        where
            create_user = "{{ target.user }}"
            and concat(table_schema, ".", table_name)
            not in (select identifier from dbt_models)
    )
select *
from final
