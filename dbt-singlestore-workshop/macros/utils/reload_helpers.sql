{# Blue/green reload. Model config: meta={'reload_strategy': {'type': 'date_window'|'hkey_mod'|'pit', 'column': '<col>'}}.
   Flow: `dbt run --target reload` (empty __new_ table + captured SQL) -> reload_batch (n x) -> `dbt test --target reload` -> reload_atomic_swap. #}

{% macro reload_state_table() %}
    {{ return("vdp_mgmt.reload_state") }}
{% endmacro %}

{% macro reload_capture(model_name, strategy, sql) %}
    {% if execute %}
        {% set state = reload_state_table() %}
        {% do run_query("create table if not exists " ~ state ~ " (model_name varchar(255) primary key, strategy_type varchar(32), strategy_column varchar(255), compiled_sql longtext, captured_at datetime)") %}
        {% set clean_sql = (sql | trim | trim(";")) | replace("\\", "\\\\") | replace("'", "''") %}
        {% do run_query("replace into " ~ state ~ " values ('" ~ model_name ~ "', '" ~ strategy["type"] ~ "', '" ~ strategy["column"] ~ "', '" ~ clean_sql ~ "', now())") %}
        {{ log("Captured compiled SQL of " ~ model_name ~ " (" ~ strategy["type"] ~ " on " ~ strategy["column"] ~ ")", info=True) }}
    {% endif %}
{% endmacro %}

{% macro get_reload_relation_names(model_name, backup_table_name=none) %}
    {% set ns = namespace(node=none) %}
    {% for node in graph.nodes.values() if node.resource_type == "model" and node.name == model_name %}
        {% set ns.node = node %}
    {% endfor %}
    {% if ns.node is none %}
        {% do exceptions.raise_compiler_error("Model " ~ model_name ~ " not found in graph.") %}
    {% endif %}
    {% set db = ns.node.database if ns.node.database else target.database %}
    {% set orig = (ns.node.alias if ns.node.alias else ns.node.name) | replace("__new_", "", 1) %}
    {% set backup = (backup_table_name | trim).split(".")[-1] if backup_table_name else orig ~ "__backup_" ~ run_started_at.strftime("%Y%m%d%H%M%S") %}
    {{ return({
        "db": db,
        "orig_identifier": orig,
        "new_identifier": "__new_" ~ orig,
        "orig": db ~ "." ~ orig,
        "new": db ~ "." ~ ("__new_" ~ orig),
        "backup": db ~ "." ~ backup
    }) }}
{% endmacro %}

{# Loads one batch (batch k of `batches`) into __new_<model>. `parts` > 1 splits the batch into smaller INSERTs (used on retry after OOM).
   date_window: start_date/end_date (end exclusive) are divided in batches*parts equal slices. hkey_mod: residues of batches*parts.
   pit: `limit` rows per batch, taken after the current max(column) of __new_<model>. #}
{% macro reload_batch(model_name, batch, batches, parts=1, start_date=none, end_date=none, limit=none) %}
    {% if execute %}
        {% set state = run_query("select strategy_type, strategy_column, compiled_sql from " ~ reload_state_table() ~ " where model_name = '" ~ model_name ~ "'") %}
        {% if state.rows | length == 0 %}
            {% do exceptions.raise_compiler_error("No captured SQL for " ~ model_name ~ ". Run `dbt run -s " ~ model_name ~ " --full-refresh --target reload` first.") %}
        {% endif %}
        {% set kind, col, src_sql = state.rows[0][0], state.rows[0][1], state.rows[0][2] %}
        {% set names = get_reload_relation_names(model_name) %}
        {% set batch, batches, parts = batch | int, batches | int, parts | int %}
        {% set head = "insert into " ~ names.new ~ " select * from (" ~ src_sql ~ ") as _reload_src where " %}

        {% for j in range(parts) %}
            {% if kind == "hkey_mod" %}
                {% set total = batches * parts %}
                {% set residue = (batch - 1) + j * batches %}
                {% set stmt = head ~ "(conv(substr(md5(cast(" ~ col ~ " as char)), 1, 8), 16, 10) % " ~ total ~ ") = " ~ residue %}
                {{ log("hkey_mod " ~ residue ~ " of " ~ total, info=True) }}
            {% elif kind == "date_window" %}
                {% if start_date is none or end_date is none %}
                    {% do exceptions.raise_compiler_error("date_window reload needs start_date and end_date.") %}
                {% endif %}
                {% set slices = batches * parts %}
                {% set idx = (batch - 1) * parts + j %}
                {% set bounds = run_query("select cast(date_add('" ~ start_date ~ "', interval floor(timestampdiff(second, '" ~ start_date ~ "', '" ~ end_date ~ "') * " ~ idx ~ " / " ~ slices ~ ") second) as char), cast(date_add('" ~ start_date ~ "', interval floor(timestampdiff(second, '" ~ start_date ~ "', '" ~ end_date ~ "') * " ~ (idx + 1) ~ " / " ~ slices ~ ") second) as char)") %}
                {% set lo, hi = bounds.rows[0][0], bounds.rows[0][1] %}
                {% set stmt = head ~ "cast(" ~ col ~ " as datetime) >= '" ~ lo ~ "' and cast(" ~ col ~ " as datetime) < '" ~ hi ~ "'" %}
                {{ log("date_window [" ~ lo ~ ", " ~ hi ~ ")", info=True) }}
            {% elif kind == "pit" %}
                {% if limit is none %}
                    {% do exceptions.raise_compiler_error("pit reload needs limit.") %}
                {% endif %}
                {% set piece = ((limit | int) / parts) | round(0, "ceil") | int %}
                {% set stmt = head ~ col ~ " > (select coalesce(max(" ~ col ~ "), 0) from " ~ names.new ~ ") order by " ~ col ~ " limit " ~ piece %}
                {{ log("pit next " ~ piece ~ " rows", info=True) }}
            {% else %}
                {% do exceptions.raise_compiler_error("Unknown reload_strategy type: " ~ kind) %}
            {% endif %}
            {% do run_query(stmt) %}
        {% endfor %}
        {% set cnt = run_query("select count(*) from " ~ names.new) %}
        {{ log("Batch " ~ batch ~ "/" ~ batches ~ " done; " ~ names.new ~ " has " ~ cnt.rows[0][0] ~ " rows", info=True) }}
    {% endif %}
{% endmacro %}

{% macro reload_atomic_swap(model_name, backup_table_name=none) %}
    {% if execute %}
        {% set names = get_reload_relation_names(model_name, backup_table_name) %}
        {% set found = run_query("select table_name, table_type from information_schema.tables where table_schema = '" ~ names.db ~ "' and table_name in ('" ~ names.orig_identifier ~ "', '" ~ names.new_identifier ~ "')") %}
        {% set types = {} %}
        {% for row in found.rows %}{% do types.update({row[0]: row[1]}) %}{% endfor %}
        {% if names.new_identifier not in types %}
            {% do exceptions.raise_compiler_error("Cannot swap: scratch table " ~ names.new ~ " does not exist.") %}
        {% endif %}
        {# SingleStore has no multi-table RENAME: sequential renames #}
        {% if names.orig_identifier in types %}
            {% if types[names.orig_identifier] == "VIEW" %}
                {% do run_query("drop view if exists " ~ names.orig) %}
            {% else %}
                {% do run_query("drop table if exists " ~ names.backup) %}
                {% do run_query("alter table " ~ names.orig ~ " rename to " ~ names.backup) %}
            {% endif %}
        {% endif %}
        {% do run_query("alter table " ~ names.new ~ " rename to " ~ names.orig) %}
        {% do run_query("delete from " ~ reload_state_table() ~ " where model_name = '" ~ model_name ~ "'") %}
        {{ log("Swapped " ~ names.new ~ " into " ~ names.orig ~ " (backup: " ~ names.backup ~ ")", info=True) }}
    {% endif %}
{% endmacro %}

{% macro reload_cleanup_scratch(model_name) %}
    {% if execute %}
        {% set names = get_reload_relation_names(model_name) %}
        {% do run_query("drop table if exists " ~ names.new) %}
        {% do run_query("delete from " ~ reload_state_table() ~ " where model_name = '" ~ model_name ~ "'") %}
        {{ log("Dropped scratch table " ~ names.new, info=True) }}
    {% endif %}
{% endmacro %}

{% macro reload_drop_backup(model_name, backup_table_name=none) %}
    {% if execute %}
        {% set names = get_reload_relation_names(model_name, backup_table_name) %}
        {% if backup_table_name %}
            {% do run_query("drop table if exists " ~ names.backup) %}
        {% else %}
            {% set old = run_query("select table_name from information_schema.tables where table_schema = '" ~ names.db ~ "' and table_name like '" ~ names.orig_identifier ~ "\\_\\_backup\\_%'") %}
            {% for row in old.rows %}
                {% do run_query("drop table if exists " ~ names.db ~ "." ~ row[0]) %}
                {{ log("Dropped backup " ~ row[0], info=True) }}
            {% endfor %}
        {% endif %}
    {% endif %}
{% endmacro %}
