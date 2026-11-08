{# Hardcoded ref because SingleStore doesn't support joining on information_schema data or views of information_schema #}
{# Must be a table, not a view #}
with
    tables as (select * from {{ source("information_schema", "TABLES") }}),
    table_stats as (
        select database_name, table_name, sum(rows) as table_rows
        from {{ source("information_schema", "TABLE_STATISTICS") }}
        where partition_type = 'Master'
        group by database_name, table_name
    ),
    table_privileges as (
        select
            gp.`ROLE` as role,
            gp.`DATABASE` as table_schema,
            gp.`TABLE` as table_name,
            gp.`PRIVILEGES` as grants
        from {{ source("information_schema", "GROUP_PRIVILEGES") }} gp
    ),
    final as (
        select
            t.table_schema,
            t.table_name,
            t.table_type,
            t.create_user,
            ts.table_rows,
            tp.role,
            tp.grants
        from tables as t
        left join
            table_stats as ts
            on t.table_schema = ts.database_name
            and t.table_name = ts.table_name
        left join
            table_privileges tp
            on t.table_schema = tp.table_schema
            and t.table_name = tp.table_name
        where
            t.table_schema
            not in ('information_schema', 'mysql', 'performance_schema', 'sys')
        order by table_schema, table_name
    )
select *
from final
