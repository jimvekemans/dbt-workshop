{% macro generate_schema_name(custom_schema_name=none, node=none) -%}

    {%- set default_schema = target.schema -%}
    {%- if target.name == "collibra" -%} {{ node.database }}
    {%- elif custom_schema_name is none -%} {{ default_schema }}
    {%- else -%} {{ custom_schema_name | trim }}
    {%- endif -%}

{%- endmacro %}
