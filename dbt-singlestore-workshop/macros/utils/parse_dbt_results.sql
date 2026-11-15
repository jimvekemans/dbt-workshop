{% macro parse_dbt_results(results) %}
    -- Create a list of parsed results
    {%- set parsed_results = [] %}
    -- Flatten results and add to list
    {% for run_result in results %}
        -- Convert the run result object to a simple dictionary
        {% set run_result_dict = run_result.to_dict() %}

        -- Get the underlying dbt graph node that was executed
        {% set node = run_result_dict.get("node") %}
        {% do log(node) %}
        {% set rows_affected = run_result_dict.get("adapter_response", {}).get(
            "rows_affected", 0
        ) %}
        {%- if not rows_affected -%} {% set rows_affected = 0 %} {%- endif -%}
        {% set resource_type = node.get("resource_type") %}
        {% set relation_name = None %}
        {% set materialized = None %}
        {% set started_at = None %}
        {% set completed_at = None %}
        {% set pre_hook = [] %}
        {% set post_hook = [] %}
        {% set compiled_code = None %}

        {% if run_result_dict.get("timing") | length > 1 %}
            {% set started_at = run_result_dict.get("timing")[1].get(
                "started_at", None
            ) %}
            {% set completed_at = run_result_dict.get("timing")[1].get(
                "completed_at", None
            ) %}
        {% endif %}

        {% if resource_type == "model" %}
            {% set relation_name = node.get("relation_name", "").replace("`", "") %}
            {% set materialized = node.get("unrendered_config").get("materialized") %}

            {% if node.get("config").get("post-hook") and node.get("config").get(
                "post-hook"
            ) | length > 0 %}
                {% for hook in node.get("config").get("pre-hook") %}
                    -- .replace('"', "'").replace("'", "''")
                    -- dbt.string_literal(
                    {{
                        pre_hook.append(
                            hook.get("sql").replace('"', "'").replace("'", "''")
                        )
                    }}
                {% endfor %}
                {% set pre_hook = ",".join(pre_hook) %}
            {% endif %}

            {% if node.get("config").get("post-hook") and node.get("config").get(
                "post-hook"
            ) | length > 0 %}
                {% for hook in node.get("config").get("post-hook") %}
                    -- .replace('"', "'").replace("'", "''")
                    -- dbt.string_literal(
                    {{
                        post_hook.append(
                            hook.get("sql").replace('"', "'").replace("'", "''")
                        )
                    }}
                {% endfor %}
                {% set post_hook = ",".join(post_hook) %}
            {% endif %}
        {% endif %}

        {% if node.get("compiled_code") %}
            {% set salt = env_var("IKL_HASH_SALT", "") %}
            {% set clean_code = node.get("compiled_code") %}
            {% if salt and salt | length > 0 %}
                {% set clean_code = clean_code.replace(salt, "***") %}
            {% endif %}
            {% set compiled_code = clean_code.replace('"', "'").replace("'", "''") %}
        {% endif %}

        {% set node_name = node.get("name") %}
        {% set parsed_result_dict = {
            "invocation_id": invocation_id,
            "run_started_at": run_started_at,
            "unique_id": node.get("unique_id"),
            "file_path": node.get("original_file_path"),
            "relation_name": relation_name,
            "database_name": node.get("database"),
            "name": node_name,
            "resource_type": resource_type,
            "materialized": materialized,
            "status": run_result_dict.get("status"),
            "failures": run_result_dict.get("failures"),
            "started_at": started_at,
            "completed_at": completed_at,
            "execution_time": run_result_dict.get("execution_time"),
            "rows_affected": rows_affected,
            "pre_hook": pre_hook,
            "post_hook": post_hook,
            "compiled_code": compiled_code,
        } %}
        {% do parsed_results.append(parsed_result_dict) %}
    {% endfor %}
    {{ return(parsed_results) }}
{% endmacro %}
