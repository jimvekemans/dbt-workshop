{% macro generate_alias_name(custom_alias_name=none, node=none) -%}
    {%- set base_name = (custom_alias_name | trim) if custom_alias_name is not none else node.name -%}
    {#- versioned models (stg_carrier_tracking_events v1/v2): one relation per version, like dbt's default `<name>_v<n>` -#}
    {%- set version = node.version if node.version is defined else none -%}
    {%- if custom_alias_name is none and version is not none and version | string != "" -%}
        {%- set base_name = base_name ~ "_v" ~ (version | string | replace(".", "_")) -%}
    {%- endif -%}
    {%- set prefix = "__new_" if (target.name == "reload" and (node.config.get("meta") or {}).get("reload_strategy")) else "" -%}
    {%- if 1 >= node.schema | length -%}
        {{ prefix ~ base_name }}
    {%- elif target.name == "collibra" -%} {{ node.name }}
    {%- else -%}
        {{ node.schema }}_{{ prefix ~ base_name }}
    {%- endif -%}
{%- endmacro %}
