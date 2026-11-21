{#- Override of dbt's built-in should_full_refresh(), which the materializations (incremental, seed, ...) and is_incremental()
    call to decide "rebuild this model from scratch". The built-in is: model config `full_refresh` when set, otherwise the
    `--full-refresh` flag. Two additions:

    1. Per model, on the command line. When the variable `full_refresh_models` (a list of model names) is given and not
       empty, exactly the listed models are fully refreshed and every other selected model runs normally, with or
       without `--full-refresh`:
           dbt build --select +fct_olist_orders+ --vars '{full_refresh_models: [stg_olist_orders, fct_olist_orders]}'
       A comma separated string works too (`--vars 'full_refresh_models: stg_olist_orders,fct_olist_orders'`). The list
       wins over the model config `full_refresh`. A name that matches no selected model does nothing.
    2. Protection. A model with `meta: {full_refresh_only_when_listed: true}` is never fully refreshed by a bare
       `--full-refresh`; only the list above rebuilds it (a model config `full_refresh` still decides first, as in dbt).
       This is for tables that hold history, e.g. dummy_load_cycles.

    Without the list and the meta key it behaves exactly like the built-in. -#}
{% macro should_full_refresh() %}
    {%- set listed = var("full_refresh_models", []) or [] -%}
    {%- if listed is string -%}
        {%- set listed = (listed | replace(" ", "")).split(",") -%}
    {%- endif -%}
    {%- if listed | length > 0 -%}
        {%- set full_refresh = (model.name if model is defined else none) in listed -%}
    {%- else -%}
        {%- set full_refresh = config.get("full_refresh") -%}
        {%- if full_refresh is none -%}
            {%- set full_refresh = flags.FULL_REFRESH -%}
            {%- if full_refresh and (config.get("meta") or {}).get(
                "full_refresh_only_when_listed"
            ) -%}
                {%- set full_refresh = false -%}
            {%- endif -%}
        {%- endif -%}
    {%- endif -%}
    {{ return(full_refresh) }}
{% endmacro %}
