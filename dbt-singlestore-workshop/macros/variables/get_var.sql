{% macro get_var(query) %}

    {% if should_execute() %} {%- set result = run_query(query) -%} {{ result.columns[0].values()[0] }}
    {% else %} 0
    {% endif %}

{% endmacro %}
