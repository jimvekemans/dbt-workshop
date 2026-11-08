{#
Deze macro genereert een lijst van alle modellen die bestaan in deze dbt run
#}
{% macro get_current_run_models() %}
  {# 
    1. Always wrap logic that depends on the graph in 'if execute' 
    to prevent errors during the initial dbt parsing phase.
  #}
  {% if execute %}
    {% set selected_models = [] %}
    
    {# selected_resources is a list of unique_ids (strings) #}
    {% for unique_id in selected_resources %}
      
      {# Get the full node object from the 'graph' context variable #}
      {% set node = graph.nodes.get(unique_id) %}

      {% if node and node.resource_type == 'model' %}
        {# Append the model's name (or other property) to your list #}
        {% do selected_models.append(node.name) %}
      {% endif %}
      
    {% endfor %}

    {{ return(selected_models) }}
  {% endif %}
  
  {{ return([]) }}
{% endmacro %}