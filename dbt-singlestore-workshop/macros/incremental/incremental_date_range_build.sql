{% macro incremental_date_range_build(dependencies, incr_start, incr_end, key, mode, filter_suffix="_incr") %}
    {%- if false %} -- fmt: off {% endif %}
    {%- if mode == 'changed_entity_full_rebuild' %}
with satellite_key_geldig_van_dt as (
    select distinct
        {{ key }}
        , x_geldig_van_dt
        {#- ,bron_sat #}
        {%- if is_incremental() %}, max(max_sat_x_geldig_van_dt_by_key) over (partition by {{ key }}) as max_x_geldig_van_dt_by_key {% endif %}
    from
        (
    {%- for dependency in dependencies %}
        {%- if not dependency.incremental_ignore -%}
        select
            {{ key }}
            , x_geldig_van_dt
            {%- if is_incremental() %}, max_x_geldig_van_dt_by_key as max_sat_x_geldig_van_dt_by_key {% endif -%}
            {#-, '{{ dependency.alias }}' as bron_sat #}
        from {{ dependency.alias ~ filter_suffix }}
            {%- if not loop.last %}
        union all
            {% endif -%}
        {% endif %}
    {%- endfor %}
        )
)
select
    dist.{{ key }},
    dist.x_geldig_van_dt,
    {#- dist.bron_sat, #} 
    {{
        generate_x_geldig_tot_dt(
            valid_from_column="dist.x_geldig_van_dt",
            partition_by=["dist." ~ key],
            order_by_columns=["dist.x_geldig_van_dt"],
        )
    }}
from satellite_key_geldig_van_dt as dist
{%- if is_incremental() %}
where
    dist.max_x_geldig_van_dt_by_key >= '{{ incr_start }}'
    and dist.max_x_geldig_van_dt_by_key < '{{ incr_end }}'
    {% endif -%}
    {% endif %}
    {%- if false %} -- fmt: on {% endif %}
{% endmacro %}
