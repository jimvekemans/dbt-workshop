{%- set model_list = [] -%}
{%- if execute -%}
    {%- for node in graph.nodes.values() | selectattr(
        "resource_type", "equalto", "model"
    ) -%}
        {%- do model_list.append(
            {
                "table_schema": node.database,
                "table_name": node.alias if node.alias else node.name,
            }
        ) -%}
    {%- endfor -%}
{%- endif -%}

{%- set model_list_json = (
    model_list | tojson | replace("\\", "\\\\") | replace("'", "''")
) -%}

{%- if not execute -%}
    select '{{ this.database }}' as table_schema, '{{ this.name }}' as table_name
{%- else -%}
    select
        json_extract_string(table_col, 'table_schema') as table_schema,
        json_extract_string(table_col, 'table_name') as table_name
    from table(json_to_array('{{ model_list_json }}'))
{%- endif -%}
