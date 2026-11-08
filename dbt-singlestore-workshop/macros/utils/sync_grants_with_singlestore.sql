{% macro sync_grants_with_singlestore(dry_run=False) %}
    {% set project_grants = {} %}
    {% for node in graph.nodes.values() | selectattr("resource_type", "equalto", "model") %}
        {% if node.config.grants %}
            {% set model_id = (node.database ~ "." ~ node.name) | lower %}
            {% set grants = node.config.grants %}
            {% do project_grants.update({model_id: grants}) %}
        {% endif %}
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

    {% set revokes = [] %}
    {% set project_grant_list = [] %}
    {% for model_id, db_grant_dict in db_grants.items() %}
        {% if project_grants.get(model_id) %}
            {% for priv, roles in project_grants[model_id].items() %}
                {% for role in roles %}
                    {% set grant = priv ~ ":" ~ role %} {% do project_grant_list.append(grant) %}
                {% endfor %}
            {% endfor %}
        {% endif %}

        {% for db_priv, db_roles in db_grant_dict.items() %}
            {% for db_role in db_roles %}
                {% set db_grant = db_priv ~ ":" ~ db_role %}
                {% if db_grant not in project_grant_list %}
                    {% set priv = db_grant.split(":")[0] %}
                    {% set role = db_grant.split(":")[1] %}
                    {% set db_parts = model_id.split(".") %}
                    {% set revoke_sql = (
                        "REVOKE "
                        ~ priv
                        ~ " ON "
                        ~ db_parts[0]
                        ~ "."
                        ~ db_parts[1]
                        ~ " FROM "
                        ~ role
                        ~ ";"
                    ) %}
                    {% do revokes.append(revoke_sql) %}
                {% endif %}
            {% endfor %}
        {% endfor %}
    {% endfor %}

    {% if revokes | length > 0 %}
        {{ log("Found " ~ revokes | length ~ " grants to revoke.", info=True) }}
        {% for revoke_sql in revokes %}
            {% if dry_run %} {{ log("[Dry Run] Would execute: " ~ revoke_sql, info=True) }}
            {% else %} {% do run_query(revoke_sql) %} {{ log("Executed: " ~ revoke_sql, info=True) }}
            {% endif %}
        {% endfor %}
    {% else %} {{ log("No grants to revoke.", info=True) }}
    {% endif %}
{% endmacro %}
