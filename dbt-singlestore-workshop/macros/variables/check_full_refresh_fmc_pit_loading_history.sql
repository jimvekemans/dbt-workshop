{% macro check_full_refresh_fmc_pit_loading_history() %}

    {% set full_refresh_requested = flags.FULL_REFRESH %}
    {% set refresh_fmc_var = var("full_refresh_fmc_pit_loading_history", default=False) %}

    {{ return(full_refresh_requested and refresh_fmc_var) }}

{% endmacro %}
