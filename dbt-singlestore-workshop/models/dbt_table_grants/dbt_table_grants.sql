with
    role_privs as (
        select
            concat('role ', `ROLE`) as grantee,
            `DATABASE` as table_schema,
            `TABLE` as table_name,
            trim(p.table_col) as privilege
        from
            {{ source("information_schema", "ROLE_PRIVILEGES") }},
            table(split(`PRIVILEGES`, ',') :> array(text)) p
    ),
    table_privs as (
        select
            replace(substring_index(grantee, '@', 1), '''', '') as grantee,
            `TABLE_SCHEMA` as table_schema,
            `TABLE_NAME` as table_name,
            `PRIVILEGE_TYPE` as privilege
        from {{ source("information_schema", "TABLE_PRIVILEGES") }}
    ),
    all_privs as (
        select *
        from role_privs
        union
        select *
        from table_privs
    ),
    dbt_tables as (
        select table_schema, table_name
        from {{ source("information_schema", "TABLES") }}
        where create_user = '{{ target.user }}'
    ),
    final as (
        select p.grantee as role, p.table_schema, p.table_name, p.privilege as grants
        from all_privs p
        where
            exists (
                select 1
                from dbt_tables t
                where p.table_schema = t.table_schema and p.table_name = t.table_name
            )
    )
select distinct
    distinct role, grants, concat(table_schema, ".", table_name) as identifier
from final
