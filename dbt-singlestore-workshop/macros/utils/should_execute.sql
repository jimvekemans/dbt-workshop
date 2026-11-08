{#
    https://docs.getdbt.com/reference/dbt-jinja-functions/execute
    execute is a Jinja variable that returns True when dbt is in "execute" mode.

    When you execute a dbt compile or dbt run command, dbt:

    Reads all of the files in your project and generates a manifest comprised of models, tests, and other graph nodes present in your project. 
    During this phase, dbt uses the ref and source statements it finds to generate the DAG for your project. 
    No SQL is run during this phase, and execute == False.
    Compiles (and runs) each node (eg. building models, or running tests). SQL is run during this phase, and execute == True.

    ---

    The flags variable contains values of flags provided on the command line.
    https://docs.getdbt.com/reference/dbt-jinja-functions/flags
#}
{% macro should_execute() %}
    {{
        return(
            execute
            and flags.WHICH
            in [
                "build",
                "run",
                "run-operation",
                "snapshot",
            ]
            and not flags.EMPTY
        )
    }}
{% endmacro %}
