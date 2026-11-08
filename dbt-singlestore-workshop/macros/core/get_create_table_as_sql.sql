{# Reload init hook: on `--target reload`, a model with `reload_strategy` is created EMPTY and its compiled SQL is captured for reload_batch. #}
{% macro get_create_table_as_sql(temporary, relation, sql) -%}
    {%- if target.name == "reload" and not temporary and (config.get("meta") or {}).get("reload_strategy") -%}
        {%- do reload_capture(model.name, config.get("meta").get("reload_strategy"), sql) -%}
        {%- set sql = "select * from (" ~ (sql | trim | trim(";")) ~ ") as _reload_src where 1 = 0" -%}
    {%- endif -%}
    {{ return(create_table_as(temporary, relation, sql)) }}
{%- endmacro %}
