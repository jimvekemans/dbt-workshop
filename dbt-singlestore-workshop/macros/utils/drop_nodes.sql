{% macro drop_nodes() %}

    {# Drops all created model/seed/snapshot relations found in the manifest for the current target (+ interrupted-run leftovers) #}
    {% set nodes = [] %}
    {% for node in graph.nodes.values() %}
        {% if node.resource_type in [
            "model",
            "seed",
            "snapshot",
        ] and node.relation_name is not none %}
            {% do nodes.append(node.relation_name) %}
            {# also the leftovers of an interrupted run (--fail-fast cancels mid-DDL): <relation>__dbt_tmp / __dbt_backup #}
            {% do nodes.append(node.relation_name[:-1] ~ "__dbt_tmp`") %}
            {% do nodes.append(node.relation_name[:-1] ~ "__dbt_backup`") %}
        {% endif %}
    {% endfor %}
    {% do log(nodes) %}

    {% if nodes | length > 0 and execute %}
        {% set existing_nodes_query %}
            with all_objects as (    
                select 
                        concat('`', table_schema, '`.`', table_name, '`') as identifier,
                        case 
                            when table_type = 'BASE TABLE' 
                            then 'TABLE'
                            else 'VIEW'
                        end as obj_type
                from information_schema.tables
                where table_schema not in ('information_schema', 'memsql', 'cluster', '{{ env_var("DBT_DEPLOY_TARGET", "dev") }}_vdp_mgmt')
            )
            select identifier, obj_type
            from all_objects
            where identifier in (
                {%- for node in nodes -%}
                    '{{ node }}'{% if not loop.last %}, {% endif %}
                {%- endfor -%}
            )
        {% endset %}

        {% set models_to_drop = run_query(existing_nodes_query) %}
        {% for model in models_to_drop.rows %}
            {%- set drop_statement -%}
            DROP {{ model['obj_type'] }} IF EXISTS {{ model['identifier'] }}
            {%- endset -%}
            {% do log(drop_statement, info=True) %}
            {% do adapter.execute(drop_statement, fetch=False) %}
        {% endfor %}
    {% endif %}

{% endmacro %}
