{#- True when the results of this run belong in the run logbook: a REAL run of build, run, test, seed or snapshot
    (`dbt retry` reports the command it repeats, so a retried build counts as build). Built on should_execute(), which
    already rejects `--empty`, the parse phase and commands that build nothing (compile, ls, parse, docs); the check-only
    command `test` and the reference-data command `seed` are added, because they write results worth keeping.
    Used by the on-run-end hook of dbt_project.yml. -#}
{% macro should_log_results() %}
    {{
        return(
            should_execute()
            or (execute and flags.WHICH in ["test", "seed"] and not flags.EMPTY)
        )
    }}
{% endmacro %}
