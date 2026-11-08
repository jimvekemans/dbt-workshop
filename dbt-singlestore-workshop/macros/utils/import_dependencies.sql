{% macro import_dependencies(dependencies_config) %}
    with
        {% for item in dependencies_config -%}
            {{ import_generate_cte(item) }} {%- if not loop.last -%}, {% endif %}
        {%- endfor -%}
{% endmacro %}
