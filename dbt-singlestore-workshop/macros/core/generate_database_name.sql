{% macro generate_database_name(custom_database_name=none, node=none) -%}
    {% set database = target.database %}
    {% if project_name == "dbt_qa" and node is not none and node.resource_type != "test" %}
        {% if custom_database_name is none %}
            {% set database = env_var("SINGLESTORE_MODEL_DB", "edw_dv_rdv") %}
        {% else %}
            {% set database = custom_database_name | trim %}
        {% endif %}
    {% elif custom_database_name is none %} {% set database = target.database %}
    {% elif target.name == "ci" %} {% set database = target.database %}

    {% else %} {% set database = custom_database_name | trim %}

    {%- endif %}
    {{ database }}
{%- endmacro %}

