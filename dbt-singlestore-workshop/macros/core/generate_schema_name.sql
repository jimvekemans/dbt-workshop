{#- Target `collibra` (data catalogue: `dbt compile --target collibra --target-path target/collibra`): the schema is the
    database the model really lives in (SingleStore has no schema level below the database), so the catalogue lists
    `<env>_edw_dv_bdv`.`<model>`. It is computed with generate_database_name instead of read from node.database, because
    dbt Fusion sets node.database only after the schema has been generated. -#}
{% macro generate_schema_name(custom_schema_name=none, node=none) -%}

    {%- set default_schema = target.schema -%}
    {%- if target.name == "collibra" -%}
        {{ generate_database_name((node.config or {}).get("database"), node) | trim }}
    {%- elif custom_schema_name is none -%} {{ default_schema }}
    {%- else -%} {{ custom_schema_name | trim }}
    {%- endif -%}

{%- endmacro %}
