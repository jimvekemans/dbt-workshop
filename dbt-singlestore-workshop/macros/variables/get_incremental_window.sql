{% macro get_incr_load_start() %}
    {% set env_start = env_var("INCR_LOAD_START", "") %}

    {% if env_start != "" %} {{ return(env_start) }}
    {% else %}
        {# default: run_started_at - 7 dagen #}
        {% set tz = var("dbt_date", {}).get("dbt_date:time_zone", "Europe/Brussels") %}
        {% set default_start = (
            (run_started_at - modules.datetime.timedelta(days=7))
            .astimezone(modules.pytz.timezone(tz))
            .strftime("%Y-%m-%d %H:%M:%S")
        ) %}
        {{ return(default_start) }}
    {% endif %}
{% endmacro %}

{% macro get_incr_load_end() %} {{ return(env_var("INCR_LOAD_END", "9999-12-31 23:59:59")) }} {% endmacro %}

{% macro get_local_run_started_at() %}
    {% set tz = var("dbt_date", {}).get("dbt_date:time_zone", "Europe/Brussels") %}
    {{ return(run_started_at.astimezone(modules.pytz.timezone(tz)).strftime("%Y-%m-%d %H:%M:%S")) }}
{% endmacro %}
