{#
    Macro om een import CTE te genereren voor een upstream afhankelijkheid.

    Dit zal hoofdzakelijk via macro 'import_dependencies' gebeuren, maar kan onafhankelijk indien meer controle nodig is.

    We zetten standaard "with (materialize = off)" om te vermijden dat SingleStore onnodig volledige brontabellen materialiseert.
    Via de optionele key "materialize_off" (default true) kan dit per dependency uitgeschakeld worden:
    {"dependency": "da_organisatie", "materialize_off": false}

    Bij 'dbt build --empty' wordt elke ref()/source() vervangen door een subquery met 'limit 0'. Dat kan bij complexe joins
    leiden tot SingleStore-fout 1749 (left side is not a sharded table). Via de optionele key "respect_empty" (default true)
    kan dit per dependency uitgeschakeld worden, zodat de volledige relatienaam gebruikt wordt (ref().render()):
    {"dependency": "da_organisatie", "respect_empty": false}

    Argument/voorbeelden: zie macro 'import_dependencies'
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
