{% macro check_full_refresh_pit_burger_db() %}

    {% set full_refresh_requested = flags.FULL_REFRESH %}
    {% set refresh_burger_var = var("full_refresh_pit_burger_db", default=False) %}

    {{ return(full_refresh_requested and refresh_burger_var) }}

{% endmacro %}
