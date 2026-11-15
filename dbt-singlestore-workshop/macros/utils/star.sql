{#
    The star macro is a utility for selecting all columns from a relation (table or view)
    except those specified in the exclude parameter.

    This is useful for:
    1. Quickly selecting all columns without having to list them individually
    2. Excluding system or metadata columns that shouldn't be part of your query
    3. Creating select statements dynamically based on the actual columns in a table

    Usage:
    ```sql
    SELECT {{ star('my_table') }} FROM {{ ref('my_table') }}
    SELECT {{ star('my_table', exclude=['sensitive_column', 'internal_id']) }} FROM {{ ref('my_table') }}
    ```

    https://docs.getdbt.com/reference/dbt-jinja-functions/get_column_names
#}
{% macro star(datasource, exclude=[]) %}
    {%- set columns = get_column_names(datasource) -%}
    {%- set filtered_columns = [] -%}

    {#- Automatically exclude snapshot metadata columns if we're in a snapshot node -#}
    {%- if datasource.resource_type == "snapshot" -%}
        {%- set exclude = exclude + [
            "dbt_scd_id",
            "dbt_updated_at",
            "dbt_valid_from",
            "dbt_valid_to",
        ] -%}
    {%- endif -%}

    {%- for col in columns -%}
        {%- if col not in exclude -%}
            {%- do filtered_columns.append(col) -%}
        {%- endif -%}
    {%- endfor -%}
    {#- relation not built yet (empty column list): fall back to * -#}
    {{ filtered_columns | join(", ") if filtered_columns else "*" }}
{% endmacro %}
