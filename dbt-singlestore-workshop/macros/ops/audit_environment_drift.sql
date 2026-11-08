{#-
    audit_environment_drift: does the live warehouse still match what Git (this project) says it should be?

        dbt run-operation audit_environment_drift --target run                                  # fails on drift
        dbt run-operation audit_environment_drift --target run --args '{fail_on_drift: false}' # report only
        dbt run-operation audit_environment_drift --args '{audit_user: svc_prd_dbt}'  # as an admin (svc_ci_dbt):
                                                     # PRD's grants next to LDV's, which svc_prd_dbt itself cannot see

    Three checks, all from information_schema - no state of its own:
      1. changed outside dbt   a table/view dbt builds (model, seed, snapshot) whose CREATE_USER or ALTER_USER is not a
                               dbt service account: a manual ALTER/CREATE in production. The next build recreates the
                               relation from Git, so the hand-made change is lost (and whoever relied on it breaks).
      2. created outside dbt   an object in a database dbt builds into that no node or source declares and that no
                               service account (or personal pu_* sandbox user) created: a hand-made hotfix object.
      3. missing grant         the user of this run (target.user, e.g. svc_prd_dbt) lacks a privilege the project
                               needs - write privileges on every database a model is built in, SELECT on every source
                               database - while the reference account (svc_ldv_dbt) has it. CI and LDV stay green;
                               only this environment fails, halfway through its build.
    Privileges are resolved user -> group -> role -> privileges (SingleStore RBAC) plus direct grants.

    dbt_release runs it before it pauses the daily load (a finding stops the release before anything is touched);
    dbt_daily_load runs it right before dbt_build with fail_on_drift=false (the build is what erases the evidence).

    vars (dbt_project.yml): drift_audit_service_accounts, drift_audit_reference_user, drift_audit_ignore_databases.
-#}
{% macro audit_environment_drift(fail_on_drift=true, reference_user=none, audit_user=none) %}
    {% if not execute %} {{ return([]) }} {% endif %}

    {% set service_accounts = var("drift_audit_service_accounts", ["svc_ci_dbt", "svc_ldv_dbt", "svc_prd_dbt"]) | map("lower") | list %}
    {% set ignore_databases = var("drift_audit_ignore_databases", ["vdp_mgmt", "information_schema"]) | map("lower") | list %}
    {% set run_user = (audit_user or target.user) | lower %}
    {% set ref_user = (reference_user or var("drift_audit_reference_user", "svc_ldv_dbt")) | lower %}
    {% set write_privileges = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "CREATE VIEW"] %}

    {# ---- what this project builds and declares (the graph = Git) ---- #}
    {% set built = {} %}
    {% set declared = [] %}
    {% set write_dbs = [] %}
    {% set read_dbs = [] %}
    {% for node in graph.nodes.values() %}
        {% if node.resource_type in ["model", "seed", "snapshot"] and node.config.materialized != "ephemeral" %}
            {% set db = node.database | lower %}
            {% do built.update({(db ~ "." ~ (node.alias or node.name)) | lower: node.unique_id}) %}
            {% if db not in write_dbs %} {% do write_dbs.append(db) %} {% endif %}
        {% endif %}
    {% endfor %}
    {% for src in graph.sources.values() %}
        {% set db = src.database | lower %}
        {% do declared.append((db ~ "." ~ (src.identifier or src.name)) | lower) %}
        {% if db not in read_dbs and db not in ignore_databases %} {% do read_dbs.append(db) %} {% endif %}
    {% endfor %}
    {% set audit_dbs = [] %}
    {% for db in write_dbs %}
        {% if db not in ignore_databases %} {% do audit_dbs.append(db) %} {% endif %}
    {% endfor %}

    {% set findings = [] %}

    {# ---- 1 + 2: relations touched by someone other than dbt ---- #}
    {% if audit_dbs %}
        {% set objects_sql %}
            select table_schema, table_name, table_type,
                   coalesce(create_user, '') as create_user,
                   coalesce(nullif(alter_user, ''), create_user, '') as alter_user,
                   cast(create_time as char) as create_time,
                   cast(coalesce(alter_time, create_time) as char) as alter_time
            from information_schema.TABLES
            where lower(table_schema) in ('{{ audit_dbs | join("', '") }}')
              and table_name not like '%\_\_dbt\_%'
              and table_name not like '\_\_new\_%'
              and table_name not like '%\_\_backup\_%'
            order by table_schema, table_name
        {% endset %}
        {% for row in run_query(objects_sql) %}
            {% set ident = (row["table_schema"] ~ "." ~ row["table_name"]) | lower %}
            {% set creator = row["create_user"] | lower %}
            {% set alterer = row["alter_user"] | lower %}
            {% if ident in built %}
                {% if creator not in service_accounts or alterer not in service_accounts %}
                    {% set who = alterer if alterer not in service_accounts else creator %}
                    {% set when = row["alter_time"] if alterer not in service_accounts else row["create_time"] %}
                    {% do findings.append({
                        "check": "changed outside dbt", "object": ident,
                        "detail": (row["table_type"] | lower) ~ " " ~ ("altered" if alterer not in service_accounts else "created")
                                  ~ " by " ~ who ~ " at " ~ when
                                  ~ ": the next build recreates it from Git and the manual change is lost",
                    }) %}
                {% endif %}
            {% elif ident not in declared and creator not in service_accounts and not creator.startswith("pu_") %}
                {% do findings.append({
                    "check": "created outside dbt", "object": ident,
                    "detail": (row["table_type"] | lower) ~ " created by " ~ creator ~ " at " ~ row["create_time"]
                              ~ ": not in the project, nobody rebuilds, tests or documents it",
                }) %}
            {% endif %}
        {% endfor %}
    {% endif %}

    {# ---- 3: effective privileges (user -> group -> role, plus direct grants) ---- #}
    {% set grants_sql %}
        select ug.`USER` as grantee, rp.`DATABASE` as db, rp.`PRIVILEGES` as privs
        from information_schema.USERS_GROUPS ug
        join information_schema.GROUPS_ROLES gr on gr.`GROUP` = ug.`GROUP`
        join information_schema.ROLE_PRIVILEGES rp on rp.`ROLE` = gr.`ROLE`
        where rp.`TABLE` = '*'
        union all
        select GRANTEE, '*', PRIVILEGE_TYPE from information_schema.USER_PRIVILEGES
        union all
        select GRANTEE, TABLE_SCHEMA, PRIVILEGE_TYPE from information_schema.SCHEMA_PRIVILEGES
    {% endset %}
    {% set privileges = {} %}
    {% for row in run_query(grants_sql) %}
        {% set user = (row["grantee"] | string).split("@")[0] | replace("'", "") | lower %}
        {% if user in [run_user, ref_user] %}
            {% set key = user ~ "|" ~ (row["db"] | lower) %}
            {% if key not in privileges %} {% do privileges.update({key: []}) %} {% endif %}
            {% for p in (row["privs"] | string).split(",") %}
                {% do privileges[key].append(p | trim | upper) %}
            {% endfor %}
        {% endif %}
    {% endfor %}

    {# without global privileges an account only sees its own rows: then the reference cannot be compared #}
    {% set ns = namespace(ref_visible=false) %}
    {% for key in privileges %}
        {% if key.split("|")[0] == ref_user %} {% set ns.ref_visible = true %} {% endif %}
    {% endfor %}

    {% set needs = [] %}
    {% for db in write_dbs %} {% do needs.append((db, write_privileges)) %} {% endfor %}
    {% for db in read_dbs %}
        {% if db not in write_dbs %} {% do needs.append((db, ["SELECT"])) %} {% endif %}
    {% endfor %}
    {% for db, required in needs %}
        {% set run_has = privileges.get(run_user ~ "|*", []) + privileges.get(run_user ~ "|" ~ db, []) %}
        {% set ref_has = privileges.get(ref_user ~ "|*", []) + privileges.get(ref_user ~ "|" ~ db, []) %}
        {% set missing = [] %}
        {% for p in required %}
            {% if p not in run_has and "ALL PRIVILEGES" not in run_has %} {% do missing.append(p) %} {% endif %}
        {% endfor %}
        {% if missing %}
            {% set ref_missing = [] %}
            {% for p in missing %}
                {% if p not in ref_has and "ALL PRIVILEGES" not in ref_has %} {% do ref_missing.append(p) %} {% endif %}
            {% endfor %}
            {% if run_user == ref_user %}
                {% set contrast = "" %}
            {% elif not ns.ref_visible %}
                {% set contrast = " (" ~ ref_user ~ "'s grants are not visible to " ~ run_user
                                  ~ ": compare as an admin, e.g. SHOW GRANTS FOR '" ~ ref_user ~ "'@'%')" %}
            {% elif not ref_missing %}
                {% set contrast = " - " ~ ref_user ~ " has them, so CI and LDV stay green" %}
            {% else %}
                {% set contrast = " - " ~ ref_user ~ " lacks " ~ (ref_missing | join(", ")) ~ " too" %}
            {% endif %}
            {% do findings.append({
                "check": "missing grant", "object": db ~ ".*",
                "detail": run_user ~ " lacks " ~ (missing | join(", ")) ~ " on " ~ db ~ contrast,
            }) %}
        {% endif %}
    {% endfor %}

    {# ---- report ---- #}
    {% do log("Environment drift audit (target " ~ target.name ~ ", user " ~ run_user ~ ", databases "
              ~ (audit_dbs | join(", ")) ~ "): " ~ (findings | length) ~ " finding(s)", info=True) %}
    {% for f in findings %}
        {% do log("  [" ~ f["check"] ~ "] " ~ f["object"] ~ ": " ~ f["detail"], info=True) %}
    {% endfor %}
    {% if findings and fail_on_drift %}
        {% do exceptions.raise_compiler_error(
            "Environment drift: " ~ (findings | length) ~ " finding(s), see above. Production no longer matches Git: "
            ~ "put the change in the project (or the grants in the grants script) first. To go ahead anyway: "
            ~ "--args '{fail_on_drift: false}'") %}
    {% endif %}
    {{ return(findings) }}
{% endmacro %}
