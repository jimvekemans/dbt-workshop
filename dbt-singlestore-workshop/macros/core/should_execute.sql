{#- True only when SQL may really be run for side effects: `execute` is on (not the parse phase), the command is one that
    builds something (build, run, run-operation, snapshot) and `--empty` is not active.
    Put it around every `run_query` in a hook or an operation that WRITES (history rows, deletes, grants). A plain
    `execute` guard is not enough: `dbt build --empty` and `dbt compile` also have `execute` on and would write rows that
    claim a load took place. `flags.WHICH` is the command that was typed, `flags.EMPTY` the `--empty` switch. -#}
{% macro should_execute() %}
    {{
        return(
            execute
            and flags.WHICH in ["build", "run", "run-operation", "snapshot"]
            and not flags.EMPTY
        )
    }}
{% endmacro %}
