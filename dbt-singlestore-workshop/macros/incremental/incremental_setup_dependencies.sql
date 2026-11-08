{# mogelijke strategiën: time_delta_active_from_source, time_delta_active_from_target, changed_entity_full_rebuild #}
{% macro incremental_setup_dependencies(dependencies, incr_start, incr_end, key, mode, filter_suffix="_incr") %}
    {% if false %} -- fmt: off {% endif %}
    {%- for dependency in dependencies %}
        {%- set incremental_ignore = dependency.incremental_ignore -%}
        {%- set dep_name = dependency.alias -%}
        {%- if not incremental_ignore -%}
{{ dep_name ~ filter_suffix}} as (
    {%- if is_incremental() %}
        {%- if mode == 'changed_entity_full_rebuild' %}
    select *, max(x_geldig_van_dt) over (partition by {{ key }}) as max_x_geldig_van_dt_by_key
    from {{ dep_name }}
    where x_geldig_van_dt < '{{ incr_end }}'
        {%- elif mode == 'time_delta_active_from_source' %}
    select *, max(x_geldig_van_dt) over (partition by {{ key }}) as max_x_geldig_van_dt_by_key
    from {{ dep_name }}
    where x_geldig_tot_dt >= '{{ incr_start }}' and x_geldig_van_dt < '{{ incr_end }}'
        {%- elif mode == 'time_delta_active_from_target' %}
    select *
    from {{ dep_name }}
    where x_geldig_van_dt >= '{{ incr_start }}' and x_geldig_van_dt < '{{ incr_end }}'
        {%- endif %}
    {%- else %}
    select *
    from {{ dep_name }}
    {%- endif %}
){%- if not loop.last -%},
{% endif %}
        {%- endif %}
    {%- endfor %}
    {% if false %} -- fmt: on {% endif %}
{% endmacro %}
