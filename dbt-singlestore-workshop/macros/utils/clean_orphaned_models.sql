{% macro clean_orphaned_models(dry_run=False) %}

    {# 1. Define the specific schemas to look for #}
    {% set env_prefix = env_var("DBT_DEPLOY_TARGET", "dev") %}
    {% set target_schemas = [
        env_prefix ~ "_edw_dv_rdv",
        env_prefix ~ "_edw_dv_bdv",
        env_prefix ~ "_im_dimensional",
    ] %}

    {# Format the list for the SQL IN clause #}
    {% set schema_list_sql = "'" + target_schemas | join("', '") + "'" %}

    {# 2. Get the list from the database (information_schema.tables) #}
    {% set get_db_models_sql %}
        SELECT DISTINCT
            lower(table_schema) as table_schema,
            lower(table_name) as table_name,
            CASE WHEN lower(table_type) like "%table"
              THEN "table"
              ELSE "view"
            END as table_type
        FROM information_schema.tables
        WHERE lower(table_schema) IN ({{ schema_list_sql }})
        AND create_user = "{{ env_var('SINGLESTORE_USER') }}"
    {% endset %}

    {# Initialize lists #}
    {% set db_models = [] %}
    {% set project_models = [] %}
    {% set orphaned_models = [] %}

    {# Only execute logic during run/compile, not parse #}
    {% if execute %}

        {# Fetch results from the database #}
        {% set results = run_query(get_db_models_sql) %}

        {# Create a list of tuples (schema, table) from the DB results #}
        {% for row in results %}
            {% do db_models.append(
                (((row["table_schema"], row["table_name"])), row["table_type"])
            ) %}
        {% endfor %}

        {# 3. Get the list of current dbt models from the 'graph' variable #}
        {% for node in graph.nodes.values() | selectattr(
            "resource_type", "equalto", "model"
        ) %}

            {% set model_name = node.name %}
            {% set model_db = node.database %}

            {# Add to project list (normalized to lowercase) #}
            {% do project_models.append((model_db | lower, model_name | lower)) %}
        {% endfor %}

        {# 4. Compare the lists: Items in DB but not in Project #}
        {% for db_item in db_models %}
            {% if db_item[0] not in project_models %}
                {% do orphaned_models.append(db_item) %}
            {% endif %}
        {% endfor %}

        {# 5. DROP the orphaned tables/views #}
        {% if orphaned_models | length > 0 %}

            {{
                log(
                    "Found " ~ orphaned_models
                    | length ~ " orphaned models. Starting cleanup...",
                    info=True,
                )
            }}

            {% for item in orphaned_models %}
                {% set db_name = item[0][0] %}
                {% set table_name = item[0][1] %}
                {% set table_type = item[1] %}

                {# 2. Construct the DROP statement based on the type (table/view) #}
                {% set drop_sql %}
                    DROP {{ table_type }} IF EXISTS {{ adapter.quote(db_name) }}.{{ adapter.quote(table_name) }};
                {% endset %}

                {# 3. Execute the Drop #}
                {% if dry_run %}
                    {# In Dry Run: Just print the SQL that WOULD run #}
                    {{ log("[Dry Run] Would execute: " ~ drop_sql | trim, info=True) }}
                {% else %}
                    {# In Live Mode: Execute and log the result #}
                    {% do run_query(drop_sql) %}
                    {{
                        log(
                            "Dropped "
                            ~ table_type
                            ~ ": "
                            ~ db_name
                            ~ "."
                            ~ table_name,
                            info=True,
                        )
                    }}
                {% endif %}

            {% endfor %}

            {{ log("Cleanup complete.", info=True) }}

        {% else %} {{ log("No orphaned models found. No actions taken.", info=True) }}
        {% endif %}

    {% endif %}

{% endmacro %}
