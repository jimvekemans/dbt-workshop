{# Hardcoded ref because SingleStore doesn't support joining on information_schema data or views of information_schema #}
{# Must be a table, not a view #}
with
    columns as (select * from {{ source("information_schema", "COLUMNS") }}),
    tables as (select * from {{ source("information_schema", "TABLES") }}),
    final as (
        select t.table_schema, t.table_name, column_name, column_type, column_key
        from columns as c
        inner join tables as t using (table_schema, table_name)
        where
            t.table_schema
            not in ('information_schema', 'mysql', 'performance_schema', 'sys')
        order by table_schema, table_name, column_name
    )
select *
from final
