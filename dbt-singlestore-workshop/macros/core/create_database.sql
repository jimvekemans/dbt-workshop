{% macro singlestore__create_database(relation) -%}
    {# no-op #}
    {%- call statement("create_database") -%} select 1 {%- endcall -%}
{% endmacro %}
