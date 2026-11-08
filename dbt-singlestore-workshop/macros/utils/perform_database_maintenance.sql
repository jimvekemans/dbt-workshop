{% macro check_database_maintenance(database_name, dry_run=True, ignore_dbt_artifacts=True) %}

    {% set created_objects_query %}
        select distinct concat(table_schema, ".", table_name) as identifier,
        case
            when table_type = 'BASE TABLE' 
            then 'table'
            else 'view'
        end as type
        from {{ source('information_schema', 'TABLES') }}
        where table_schema = '{{ database_name }}'
        and create_user = '{{ target.user }}'
        {%- if ignore_dbt_artifacts -%} and table_name not like '%__dbt_%'{%- endif -%}
    {% endset %}

    {% set created_objects = [] %}
    {% if execute %}
        {% set results = run_query(created_objects_query) %}
        {% for row in results %}
            {% set obj = {
                "identifier": row["identifier"],
                "type": row["type"],
            } %}
            {% do created_objects.append(obj) %}
        {% endfor %}
    {% endif %}

    {% set dbt_objects = [] %}
    {% for node in graph.nodes.values() %}
        {% if node.resource_type in ["model", "seed", "snapshot"] and node.database == database_name %}
            {% do dbt_objects.append(node.database ~ "." ~ node.name) %}
        {% endif %}
    {% endfor %}

    {% set orphaned_objects = [] %}
    {% for obj in created_objects %}
        {% if obj["identifier"] not in dbt_objects %} {% do orphaned_objects.append(obj) %} {% endif %}
    {% endfor %}

    {% set grants_query %}
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
        select distinct distinct role, grants, concat(table_schema, ".", table_name) as identifier
        from final
        where table_schema = '{{ database_name }}'
    {% endset %}

    {% set db_grants = {} %}
    {% if execute %}
        {% set results = run_query(grants_query) %}
        {% for row in results %}
            {% set model_id = (row["identifier"]) | lower %}
            {% set priv = row["grants"] | lower %}
            {% set role = row["role"] | lower %}
            {% if db_grants.get(model_id) is none %} {% do db_grants.update({model_id: {}}) %} {% endif %}
            {% if db_grants[model_id].get(priv) is none %}
                {% do db_grants[model_id].update({priv: []}) %}
            {% endif %}
            {% if role not in db_grants[model_id][priv] %}
                {% do db_grants[model_id][priv].append(role) %}
            {% endif %}
        {% endfor %}
    {% endif %}

    {% set drop_commands = [] %}
    {% set revokes = [] %}
    {% for obj in orphaned_objects %}
        {% set identifier = obj.get("identifier") %}
        {% set drop_command = "drop " ~ obj.get("type") ~ " if exists " ~ identifier ~ ";" %}
        {% do drop_commands.append(drop_command) %}
        {% set db_grant_dict = db_grants.get(identifier) %}
        {% if db_grant_dict %}
            {% for priv, db_roles in db_grant_dict.items() %}
                {% for role in db_roles %}
                    {% set revoke_sql = "revoke " ~ priv ~ " on " ~ identifier ~ " from " ~ role ~ ";" %}
                    {% do revokes.append(revoke_sql) %}
                {% endfor %}
            {% endfor %}
        {% endif %}
    {% endfor %}

    {% if not dry_run %}
        {% for revoke_sql in revokes %}
            {% do run_query(revoke_sql) %} {{ log("Executed: " ~ revoke_sql, info=True) }}
        {% endfor %}
        {% for drop_sql in drop_commands %}
            {% do run_query(drop_sql) %} {{ log("Executed: " ~ drop_sql, info=True) }}
        {% endfor %}
    {% endif %}

    {{ return({"drops": drop_commands, "revokes": revokes}) }}
{% endmacro %}

{% macro perform_database_maintenance() %}
    {% set maintenance_report = {} %}
    {% for db in ["edw_dv_bdv", "edw_fmc", "edw_ldm", "pres_dataproduct", "im_dimensional"] %}
        {% set result = check_database_maintenance(db, dry_run=false, ignore_dbt_artifacts=true) %}
        {% do maintenance_report.update([(db, result)]) %}
    {% endfor %}
    {{ print(maintenance_report | tojson) }}
{% endmacro %}
