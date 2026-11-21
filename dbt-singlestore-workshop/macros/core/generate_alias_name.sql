{#- Target `collibra`: the plain model name, without user or branch prefix and without the `__new_` of the reload target
    (the relation name as it really is in the database; see generate_schema_name). -#}
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
    {%- if target.name == "collibra" -%} {{ base_name }}
    {%- elif 1 >= node.schema | length -%} {{ prefix ~ base_name }}
    {%- else -%} {{ node.schema }}_{{ prefix ~ base_name }}
    {%- endif -%}
{%- endmacro %}
