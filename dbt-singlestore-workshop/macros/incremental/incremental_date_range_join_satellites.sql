{% macro incremental_date_range_join_satellites(dependencies, key, join_key, mode, filter_suffix="_incr", date_range_alias="date_range") %}
    {% if false %} -- fmt: off {% endif %}
    {%- if mode == 'changed_entity_full_rebuild' %}
    {%- for dependency in dependencies %}
        {% if not dependency.incremental_ignore %}
    left join
        {{ dependency.alias }} as {{ dependency.abbreviation }}
        on (
            {{ join_key }} = {{ dependency.abbreviation }}.{{ key }}
            and {{date_range_alias}}.x_geldig_van_dt >= {{ dependency.abbreviation }}.x_geldig_van_dt
            and {{date_range_alias}}.x_geldig_van_dt < {{ dependency.abbreviation }}.x_geldig_tot_dt
        )
        {% endif %}
    {%- endfor %}
    {% endif %}
    {% if false %} -- fmt: on {% endif %}
{% endmacro %}




