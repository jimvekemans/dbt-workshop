{% macro table_exists(database, name) %} 
    {%- if execute -%}           
        
        {% set target_relation = adapter.get_relation(
            database=database,
            schema=target.schema,
            identifier=name
        ) %}

        {% if target_relation is not none %}
            {% do return(True) %}
        {% else %}
            {% do return(False) %}
        {% endif %}
    {% endif %}
{% endmacro %}
