CREATE SEQUENCE IF NOT EXISTS statement_id_seq START 1;
CREATE SEQUENCE IF NOT EXISTS object_id_seq START 1;

CREATE TABLE IF NOT EXISTS statements (
    database          VARCHAR,
    schema            VARCHAR,
    name              VARCHAR,
    identifier        VARCHAR,
    raw_sql           VARCHAR NOT NULL,
    formatted_sql     VARCHAR NOT NULL,
    creation_time     TIMESTAMPTZ NOT NULL,
    run_time          TIMESTAMPTZ NOT NULL,
    success           BOOLEAN,
    rows_affected     BIGINT,
    statement_id      BIGINT PRIMARY KEY,
    object_id         BIGINT,
    object_type       VARCHAR,
    operation         VARCHAR NOT NULL,
    statement_kind    VARCHAR NOT NULL,
    column_name       VARCHAR,
    renamed_to        VARCHAR,
    outcome           VARCHAR NOT NULL,
    decision          VARCHAR,
    error_code        INTEGER,
    error_message     VARCHAR,
    duration_ms       BIGINT,
    effective_sql     VARCHAR NOT NULL,
    raw_fp            VARCHAR NOT NULL,
    effective_fp      VARCHAR NOT NULL,
    applied_rules     VARCHAR[],
    environment       VARCHAR NOT NULL,
    run_id            VARCHAR,
    source            VARCHAR,
    job               VARCHAR,
    rel_path          VARCHAR,
    group_id          VARCHAR,
    phase             VARCHAR,
    statement_index   INTEGER,
    ledger_sequence   BIGINT,
    git_sha           VARCHAR,
    recorded_at       TIMESTAMPTZ NOT NULL DEFAULT current_timestamp
);
COMMENT ON TABLE statements IS 'One row per statement the framework handled, in any outcome. SingleStore has no level between database and table, so "schema" always equals "database". "success" is NULL when nothing ran (skipped, refused, not_run).';

CREATE TABLE IF NOT EXISTS objects (
    object_id             BIGINT PRIMARY KEY,
    environment           VARCHAR NOT NULL,
    object_type           VARCHAR NOT NULL,
    family                VARCHAR NOT NULL,
    database              VARCHAR,
    name                  VARCHAR NOT NULL,
    identifier            VARCHAR NOT NULL,
    first_identifier      VARCHAR NOT NULL,
    origin                VARCHAR NOT NULL,
    created_statement_id  BIGINT,
    created_at            TIMESTAMPTZ,
    status                VARCHAR NOT NULL,
    ended_at              TIMESTAMPTZ,
    ended_statement_id    BIGINT
);
COMMENT ON TABLE objects IS 'One row per object lifespan. "origin" is untracked when the first statement seen was not a CREATE. Identity survives renames; "identifier" is the current name.';

CREATE TABLE IF NOT EXISTS object_names (
    object_id          BIGINT NOT NULL,
    database           VARCHAR,
    name               VARCHAR NOT NULL,
    identifier         VARCHAR NOT NULL,
    valid_from         TIMESTAMPTZ NOT NULL,
    valid_to           TIMESTAMPTZ,
    from_statement_id  BIGINT,
    to_statement_id    BIGINT
);
COMMENT ON TABLE object_names IS 'Name history of each object; valid_to is NULL for the current name.';

CREATE OR REPLACE VIEW live_objects AS
SELECT * FROM objects WHERE status = 'live';

CREATE OR REPLACE VIEW object_lifespan AS
SELECT o.identifier AS object_identifier, o.status AS object_status, o.origin AS object_origin, s.*
FROM statements s
JOIN objects o USING (object_id)
ORDER BY s.object_id, s.run_time, s.statement_id;

CREATE OR REPLACE VIEW rename_history AS
SELECT object_id, environment, identifier AS from_identifier, renamed_to AS to_identifier,
       run_time, statement_id
FROM statements
WHERE renamed_to IS NOT NULL AND success IS TRUE
ORDER BY object_id, run_time, statement_id;
