{#- Loading history of one model: a row per load cycle in a history model (here `dummy_load_cycles`).
    open_load_cycle (pre-hook) adds the row with success_flag 0 and no end time, close_load_cycle (post-hook) marks the
    newest open row of the object as successful and sets its end time. A build that fails leaves its row open, so
    "open" means "started, never finished".
    The guard is the one `should_execute()` per macro: it keeps `--empty` and other dry commands from writing a row that
    claims a load happened. The cycle is also skipped on target `ci` (a verify build may defer the history table to a
    database the CI account cannot write) and `reload` (the blue/green reload keeps its own state), and when the history
    table does not exist yet in the database.
    The table name is built from the parts of the relation: under `--empty` dbt renders a ref() as a `limit 0` subquery,
    which is no table name for an INSERT or UPDATE. -#}
{% macro open_load_cycle(history, object_name) %}
    {%- if should_execute() and target.name not in ["ci", "reload"] -%}
        {%- if load_relation(history) is none -%}
            {{
                log(
                    "open_load_cycle: "
                    ~ history.identifier
                    ~ " does not exist, no load cycle opened for "
                    ~ object_name,
                    info=True,
                )
            }}
        {%- else -%}
            {%- set table = api.Relation.create(
                database=history.database,
                schema=history.schema,
                identifier=history.identifier,
            ) -%}
            {%- set last = run_query(
                "select coalesce(max(load_cycle_id), 0) from "
                ~ table
                ~ " where object_name = '"
                ~ object_name
                ~ "'"
            ) -%}
            {%- do run_query(
                "insert into "
                ~ table
                ~ " (load_cycle_id, object_name, invocation_id, load_start_date, load_end_date, success_flag) values ("
                ~ (last.rows[0][0] | int + 1)
                ~ ", '"
                ~ object_name
                ~ "', '"
                ~ invocation_id
                ~ "', now(6), null, 0)"
            ) -%}
        {%- endif -%}
    {%- endif -%}
{% endmacro %}

{% macro close_load_cycle(history, object_name) %}
    {%- if should_execute() and target.name not in ["ci", "reload"] -%}
        {%- if load_relation(history) is not none -%}
            {%- set table = api.Relation.create(
                database=history.database,
                schema=history.schema,
                identifier=history.identifier,
            ) -%}
            {%- set last = run_query(
                "select coalesce(max(load_cycle_id), 0) from "
                ~ table
                ~ " where object_name = '"
                ~ object_name
                ~ "'"
            ) -%}
            {%- do run_query(
                "update "
                ~ table
                ~ " set load_end_date = now(6), success_flag = 1 where object_name = '"
                ~ object_name
                ~ "' and success_flag = 0 and load_cycle_id = "
                ~ (last.rows[0][0] | int)
            ) -%}
        {%- endif -%}
    {%- endif -%}
{% endmacro %}
