{% macro pre_hook_fmc_pit_loading_history(fmc_pit_loading_history) %}
    {%- set schema_prefix = target.schema ~ "_" if target.schema != "" else "" -%}
    {%- if check_full_refresh_pit_burger_db() -%}
        {%- set final_object_name = schema_prefix ~ "pit_burger_db" -%}
        {%- set prep_loading_history_query -%}
            DELETE FROM {{ fmc_pit_loading_history }} where object_name = '{{ final_object_name }}';
        {%- endset -%}
        {% if should_execute() %}
            {%- do run_query(prep_loading_history_query) -%}
        {% endif %}
    {% endif %}
    {%- if check_full_refresh_pit_burger_niet_beschikbaarheid_db() -%}
        {%- set final_object_name = (
            schema_prefix ~ "pit_burger_niet_beschikbaarheid_db"
        ) -%}
        {%- set prep_loading_history_query -%}
            DELETE FROM {{ fmc_pit_loading_history }} where object_name = '{{ final_object_name }}';
        {%- endset -%}
        {% if should_execute() %}
            {%- do run_query(prep_loading_history_query) -%}
        {% endif %}
    {% endif %}
    {%- if check_full_refresh_pit_medewerker_db() -%}
        {%- set final_object_name = schema_prefix ~ "pit_medewerker_db" -%}
        {%- set prep_loading_history_query -%}
            DELETE FROM {{ fmc_pit_loading_history }} where object_name = '{{ final_object_name }}';
        {%- endset -%}
        {% if should_execute() %}
            {%- do run_query(prep_loading_history_query) -%}
        {% endif %}
    {% endif %}
{% endmacro %}
