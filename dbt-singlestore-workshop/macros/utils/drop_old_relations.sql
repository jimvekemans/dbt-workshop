{% macro drop_old_relations(relations_to_drop) %}
    {% if relations_to_drop is iterable and relations_to_drop|length > 0 %}
        {% for relation in relations_to_drop %}
            {% set parts = relation.split('.') %}
            {# parts[0] = db, parts[1] = schema, parts[2] = table #}
            
            {% set rel = adapter.get_relation(database=parts[0], schema=parts[1], identifier=parts[2]) %}
            
            {% if rel %}
                {{ log("DROPPING: " ~ rel, info=True) }}
                {% do adapter.drop_relation(rel) %}
            {% else %}
                {{ log("NOT FOUND (Already gone): " ~ relation, info=True) }}
            {% endif %}
        {% endfor %}
    {% else %}
        {{ log("No relations to drop", info=True) }}
    {% endif %}
{% endmacro %}