{#
    Macro that generates an import CTE for an upstream dependency.

    This mostly happens through the macro 'import_dependencies', but it can be called on its own for more control.

    By default we add "with (materialize = off)" so SingleStore does not needlessly materialize whole source tables.
    The optional key "materialize_off" (default true) disables it per dependency:
    {"dependency": "da_organisatie", "materialize_off": false}

    With 'dbt build --empty' every ref()/source() is replaced by a 'limit 0' subquery. For complex joins that can cause
    SingleStore error 1749 (left side is not a sharded table). The optional key "respect_empty" (default true)
    disables this per dependency so the full relation name is used (ref().render()):
    {"dependency": "da_organisatie", "respect_empty": false}

    Arguments/examples: see macro 'import_dependencies'
#}
{% macro import_generate_cte(item) %}
    {%- set is_dict = item is mapping -%}
    {%- set dependency_name = item.dependency if is_dict else item -%}
    {%- set source_name = item.source if is_dict and item.source else none -%}
    {%- set alias = item.alias if is_dict and item.alias else dependency_name -%}
    {%- set materialize_off = item.get("materialize_off", true) if is_dict else true -%}
    {%- set respect_empty = item.get("respect_empty", true) if is_dict else true -%}
    {%- set materialize_hint = (
        "with (materialize = off) " if materialize_off else ""
    ) -%}
    {{ alias }} as (
        select {{ materialize_hint }}*
        from
            {% if source_name -%}
                {{
                    (
                        source(source_name, dependency_name)
                        if respect_empty
                        else source(source_name, dependency_name).render()
                    )
                }}
            {%- else -%}
                {{
                    (
                        ref(dependency_name)
                        if respect_empty
                        else ref(dependency_name).render()
                    )
                }}
            {%- endif %}
    )
{% endmacro %}
