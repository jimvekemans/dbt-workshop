{#  Verwerk de dependencies lijst om alle impliciete opties expliciet te maken.
    Dit vermijdt die logica in elke macro die er gebruik van maakt te implementeren.

    Wijzigingen:
    - vormt de verkorte versie (string) om naar een dictionary
    - voegt toe indien ontbreken: alias & abbreviation

    Alias: gebruikt bij importeren. Doel: wanneer een bron meerdere keren nodig is.
    Abbreviation: gebruikt bij joinen aan date range.
#}
{% macro incremental_normalize_dependencies(dependencies) %}
    {%- set normalized = [] -%}

    {%- for item in dependencies -%}
        {%- if item is mapping -%} {% set normalized_item = item.copy() %}
        {% else %} {% set normalized_item = {"dependency": item} %}
        {% endif %}
        {% if "alias" not in normalized_item %}
            {% do normalized_item.update({"alias": normalized_item["dependency"]}) %}
        {% endif %}
        {% if "abbreviation" not in normalized_item %}
            {% do normalized_item.update({"abbreviation": normalized_item["alias"]}) %}
        {% endif %}
        {%- do normalized.append(normalized_item) -%}
    {%- endfor -%}

    {{ return(normalized) }}
{% endmacro %}
