{% macro singlestore__create_database(relation) -%}
    {# no-op #}
    {%- call statement('create_database') -%}
        SELECT 1
    {%- endcall -%}
{% endmacro %}