{% macro log_dbt_results(results) %}

    {%- if execute -%}

        {%- set table_identifier = api.Relation.create(
            database=env_var("DBT_DEPLOY_TARGET", "dev") ~ "_vdp_mgmt",
            identifier="dbt_run_results",
        ) %}

        {%- set parsed_results = parse_dbt_results(results) -%}
        {%- if parsed_results | length > 0 -%}
            {% set insert_dbt_results_query -%}
                insert into {{ table_identifier }}
                    (
                        invocation_id,
                        run_started_at,                        
                        unique_id,
                        file_path,
                        relation_name,
                        database_name,
                        name,
                        resource_type,
                        materialized,
                        status,
                        failures,
                        execution_time,
                        started_at,
                        completed_at,
                        rows_affected,
                        pre_hook,
                        post_hook,
                        compiled_code
                ) values
                    {%- for parsed_result_dict in parsed_results -%}
                        (
                            '{{ parsed_result_dict.get('invocation_id') }}',
                            '{{ parsed_result_dict.get('run_started_at') }}',
                            '{{ parsed_result_dict.get('unique_id') }}',
                            '{{ parsed_result_dict.get('file_path') }}',
                            '{{ parsed_result_dict.get('relation_name') }}',
                            '{{ parsed_result_dict.get('database_name') }}',
                            '{{ parsed_result_dict.get('name') }}',
                            '{{ parsed_result_dict.get('resource_type') }}',
                            '{{ parsed_result_dict.get('materialized') }}',
                            '{{ parsed_result_dict.get('status') }}',
                            '{{ parsed_result_dict.get('failures') }}',
                            '{{ parsed_result_dict.get('execution_time') }}',
                            '{{ parsed_result_dict.get('started_at') }}',
                            '{{ parsed_result_dict.get('completed_at') }}',
                            '{{ parsed_result_dict.get('rows_affected') }}',
                            '{{ parsed_result_dict.get('pre_hook') }}',
                            '{{ parsed_result_dict.get('post_hook') }}',
                            '{{ parsed_result_dict.get('compiled_code') }}'
                        ) {{- "," if not loop.last else "" -}}
                    {%- endfor -%}
            {%- endset -%}
            {%- do run_query(insert_dbt_results_query) -%}
        {%- endif -%}
    {%- endif -%}

    {{ return("") }}
{% endmacro %}
