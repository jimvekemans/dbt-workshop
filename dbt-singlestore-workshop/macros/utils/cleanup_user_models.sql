{#
    This macro cleans the models created during development (pu-prefixed objects).
    Call the macro with the run-operation command:
    $ dbt run-operation cleanup_user_models

    If you want to keep your recent pu-models and drop old models then use this command:
    $ dbt run-operation cleanup_user_models --args \'{keep_younger_than_days: 7}\'
#}
{% macro cleanup_user_models(all_pu_users=False, keep_younger_than_days=0) %}
    {% set user_artifacts %}
        select 
            concat('`', table_schema, '`.`', table_name, '`') as identifier,
            case 
                when table_type = 'BASE TABLE' 
                then 'table'
                else 'view'
            end as type
        from information_schema.tables
        where table_schema in ('dev_edw_dv_rdv', 'dev_edw_dv_bdv', 'dev_im_dimensional')
        and create_time < date_sub(NOW(), interval {{keep_younger_than_days}} DAY)
        and table_name like '{%- if all_pu_users | as_bool -%}pu%{%- else -%}{{ env_var("SINGLESTORE_USER") }}{%- endif-%}_%'
    {%- endset %}
    {% do log(user_artifacts, True) %}
    {% set identifiers = run_query(user_artifacts).columns[0].values() %}
    {% set types = run_query(user_artifacts).columns[1].values() %}
    {% if identifiers %}
        {% for id in range(identifiers | length) %}
            {% set item = identifiers[id] %}
            {% set type = types[id] %}
            {% set drop_command = "drop " ~ type ~ " if exists " ~ item %}
            {% do log(drop_command, True) %} {% do run_query(drop_command) %}
        {% endfor %}
    {% else %} {% do log("No relations to clean.", True) %}
    {% endif %}
{%- endmacro -%}
