with
    dbt_models as (select * from {{ ref("dbt_models") }}),
    dwh_table_info as (select * from {{ ref("singlestore_table_info") }}),
    dwh_column_info as (select * from {{ ref("singlestore_column_info") }}),
    dwh_info as (
        select ti.*, ci.column_name, ci.column_type, ci.column_key
        from dwh_table_info ti
        join
            dwh_column_info ci
            on ti.table_schema = ci.table_schema
            and ti.table_name = ci.table_name
    ),
    final as (
        select b.*
        from dbt_models a
        join dwh_info b
        where a.table_schema = b.table_schema and a.table_name = b.table_name
        order by b.table_schema, b.table_name, column_name
    )
select *
from final
