{% macro check_full_refresh_pit_burger_indicatie_arbeidsbeperking_db() %}

    {% set full_refresh_requested = flags.FULL_REFRESH %}
    {% set refresh_var = var(
        "full_refresh_pit_burger_indicatie_arbeidsbeperking_db", default=False
    ) %}

    {{ return(full_refresh_requested and refresh_var) }}

{% endmacro %}
