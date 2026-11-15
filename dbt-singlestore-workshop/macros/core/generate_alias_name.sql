{% macro generate_alias_name(custom_alias_name=none, node=none) -%}
    {%- set base_name = (
        (custom_alias_name | trim)
        if custom_alias_name is not none
        else node.name
    ) -%}
    {%- set prefix = (
        "__new_"
        if (
            target.name == "reload"
            and (node.config.get("meta") or {}).get("reload_strategy")
        )
        else ""
    ) -%}
    {%- if 1 >= node.schema | length -%} {{ prefix ~ base_name }}
    {%- else -%} {{ node.schema }}_{{ prefix ~ base_name }}
    {%- endif -%}
{%- endmacro %}
