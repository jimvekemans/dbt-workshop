{% macro check_full_refresh_pit_medewerker_db() %}
    {{ return(flags.FULL_REFRESH and var("full_refresh_pit_medewerker_db", false)) }}
{% endmacro %}
