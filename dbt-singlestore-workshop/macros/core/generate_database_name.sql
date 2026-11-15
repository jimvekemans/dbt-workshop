{#- Database naming: target `ci` always builds into vdp_ci; otherwise a custom database X becomes <env>_X
    (env = DBT_DEPLOY_TARGET: dev | val | prd) and a model without one lands in target.database. -#}
{% macro generate_database_name(custom_database_name=none, node=none) -%}
    {%- if target.name == "ci" -%} vdp_ci
    {%- elif custom_database_name is none -%} {{ target.database }}
    {%- else -%}
        {{ env_var("DBT_DEPLOY_TARGET", "dev") }}_{{ custom_database_name | trim }}
    {%- endif -%}
{%- endmacro %}
