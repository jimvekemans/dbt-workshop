//! Recording one event: the statement row plus the object bookkeeping, in one transaction.

use chrono::{DateTime, SecondsFormat, Utc};
use duckdb::types::ToSql;
use duckdb::{Connection, Transaction, params};
use sqlformat::{FormatOptions, Indent, QueryParams};
use vdp_vaultspeed_domain::{ChangeEvent, ChangeOutcome, ObjectRef, ObjectType, StatementKind};

use crate::DuckDbHistoryError;

/// Separator for list values passed as one bound parameter; cannot occur in a rule id.
const LIST_SEP: char = '\u{1f}';

pub(crate) fn record_event(
    conn: &mut Connection,
    event: &ChangeEvent,
) -> Result<(), DuckDbHistoryError> {
    let tx = conn.transaction()?;
    let statement_id: i64 = tx.query_row("SELECT nextval('statement_id_seq')", [], |r| r.get(0))?;
    let object_id = resolve_object(&tx, event, statement_id)?;
    insert_statement(&tx, event, statement_id, object_id)?;
    tx.commit()?;
    Ok(())
}

fn family(object_type: ObjectType) -> &'static str {
    match object_type {
        ObjectType::Table | ObjectType::View => "relation",
        other => other.as_str(),
    }
}

fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn operation(kind: &StatementKind) -> &'static str {
    match kind {
        StatementKind::Create { .. } => "CREATE",
        StatementKind::Drop { .. } => "DROP",
        StatementKind::Alter { .. } => "ALTER",
        StatementKind::Truncate => "TRUNCATE",
        StatementKind::Insert => "INSERT",
        StatementKind::Update => "UPDATE",
        StatementKind::Delete => "DELETE",
        StatementKind::Merge => "MERGE",
        StatementKind::Replace => "REPLACE",
        StatementKind::Call => "CALL",
        StatementKind::Set => "SET",
        StatementKind::Select => "SELECT",
        StatementKind::Use => "USE",
        StatementKind::Other => "OTHER",
    }
}

fn format_sql(sql: &str) -> String {
    let options = FormatOptions {
        indent: Indent::Spaces(2),
        uppercase: Some(true),
        lines_between_queries: 1,
        ..FormatOptions::default()
    };
    sqlformat::format(sql, &QueryParams::None, &options)
}

/// The rename target, qualified with the source's database when written unqualified.
fn rename_target(event: &ChangeEvent) -> Option<ObjectRef> {
    let to = event.renamed_to()?;
    let schema = to
        .schema
        .clone()
        .or_else(|| event.target.as_ref().and_then(|t| t.schema.clone()));
    Some(ObjectRef {
        object_type: to.object_type,
        schema,
        name: to.name.clone(),
    })
}

fn live_object(
    tx: &Transaction<'_>,
    environment: &str,
    target: &ObjectRef,
) -> Result<Option<i64>, DuckDbHistoryError> {
    let found = tx.query_row(
        "SELECT max(object_id) FROM objects \
             WHERE environment = ? AND family = ? AND identifier = ? AND status = 'live'",
        params![
            environment,
            family(target.object_type),
            target.qualified_name()
        ],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    Ok(found)
}

/// Starts a lifespan. `created_by` is the CREATE statement; `None` marks an untracked object, whose first
/// name is valid from the moment it was first seen.
fn start_object(
    tx: &Transaction<'_>,
    event: &ChangeEvent,
    target: &ObjectRef,
    created_by: Option<i64>,
) -> Result<i64, DuckDbHistoryError> {
    let object_id: i64 = tx.query_row("SELECT nextval('object_id_seq')", [], |r| r.get(0))?;
    let identifier = target.qualified_name();
    let time = timestamp(event.run_time);
    let origin = if created_by.is_some() {
        "created"
    } else {
        "untracked"
    };
    let created_at = created_by.map(|_| time.clone());
    tx.execute(
        "INSERT INTO objects (object_id, environment, object_type, family, database, name, identifier, \
         first_identifier, origin, created_statement_id, created_at, status) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CAST(? AS TIMESTAMPTZ), 'live')",
        params![
            object_id,
            event.environment,
            target.object_type.as_str(),
            family(target.object_type),
            target.schema,
            target.name,
            identifier,
            identifier,
            origin,
            created_by,
            created_at
        ],
    )?;
    add_name(tx, object_id, target, &time, created_by)?;
    Ok(object_id)
}

fn add_name(
    tx: &Transaction<'_>,
    object_id: i64,
    name: &ObjectRef,
    time: &str,
    from_statement: Option<i64>,
) -> Result<(), DuckDbHistoryError> {
    tx.execute(
        "INSERT INTO object_names (object_id, database, name, identifier, valid_from, from_statement_id) \
         VALUES (?, ?, ?, ?, CAST(? AS TIMESTAMPTZ), ?)",
        params![
            object_id,
            name.schema,
            name.name,
            name.qualified_name(),
            time,
            from_statement
        ],
    )?;
    Ok(())
}

fn close_name(
    tx: &Transaction<'_>,
    object_id: i64,
    time: &str,
    statement_id: i64,
) -> Result<(), DuckDbHistoryError> {
    tx.execute(
        "UPDATE object_names SET valid_to = CAST(? AS TIMESTAMPTZ), to_statement_id = ? \
         WHERE object_id = ? AND valid_to IS NULL",
        params![time, statement_id, object_id],
    )?;
    Ok(())
}

fn end_object(
    tx: &Transaction<'_>,
    object_id: i64,
    time: DateTime<Utc>,
    statement_id: i64,
) -> Result<(), DuckDbHistoryError> {
    let time = timestamp(time);
    tx.execute(
        "UPDATE objects SET status = 'dropped', ended_at = CAST(? AS TIMESTAMPTZ), ended_statement_id = ? \
         WHERE object_id = ?",
        params![time, statement_id, object_id],
    )?;
    close_name(tx, object_id, &time, statement_id)
}

/// Finds the object the statement belongs to and, when the statement took effect, applies its effect on the
/// object's lifespan.
fn resolve_object(
    tx: &Transaction<'_>,
    event: &ChangeEvent,
    statement_id: i64,
) -> Result<Option<i64>, DuckDbHistoryError> {
    let Some(target) = &event.target else {
        return Ok(None);
    };
    let live = live_object(tx, &event.environment, target)?;
    if !event.outcome.took_effect() {
        return Ok(live);
    }
    if event.outcome == ChangeOutcome::Satisfied {
        return resolve_satisfied(tx, event, target, live, statement_id);
    }
    let time = event.run_time;
    match &event.kind {
        StatementKind::Create {
            object,
            or_replace,
            if_not_exists,
        } => {
            if let Some(id) = live {
                if *or_replace || *if_not_exists || object.is_replaceable() {
                    tx.execute(
                        "UPDATE objects SET object_type = ? WHERE object_id = ?",
                        params![target.object_type.as_str(), id],
                    )?;
                    return Ok(Some(id));
                }
                // A plain CREATE over a live table: it was dropped outside the framework.
                end_object(tx, id, time, statement_id)?;
            }
            start_object(tx, event, target, Some(statement_id)).map(Some)
        }
        StatementKind::Drop { .. } => match live {
            Some(id) => {
                end_object(tx, id, time, statement_id)?;
                Ok(Some(id))
            }
            None => {
                let id = start_object(tx, event, target, None)?;
                end_object(tx, id, time, statement_id)?;
                Ok(Some(id))
            }
        },
        kind => {
            let id = match live {
                Some(id) => id,
                None => start_object(tx, event, target, None)?,
            };
            if let (StatementKind::Alter { .. }, Some(to)) = (kind, rename_target(event)) {
                rename_object(tx, event, id, &to, statement_id)?;
            }
            Ok(Some(id))
        }
    }
}

/// A satisfied statement did not change the database: its desired state already held (the table already
/// existed, the column was already gone, the rename had already happened). It never starts a lifespan of its
/// own; it confirms what lives there. A CREATE over nothing known means the object was made outside the
/// framework (`untracked`); a DROP of something live means it went away outside the framework; a rename whose
/// source is unknown links to whatever lives under the target.
fn resolve_satisfied(
    tx: &Transaction<'_>,
    event: &ChangeEvent,
    target: &ObjectRef,
    live: Option<i64>,
    statement_id: i64,
) -> Result<Option<i64>, DuckDbHistoryError> {
    if let StatementKind::Drop { .. } = event.kind {
        if let Some(id) = live {
            end_object(tx, id, event.run_time, statement_id)?;
        }
        return Ok(live);
    }
    if let (StatementKind::Alter { .. }, Some(to)) = (&event.kind, rename_target(event)) {
        return match live {
            Some(id) => rename_object(tx, event, id, &to, statement_id).map(|()| Some(id)),
            None => live_object(tx, &event.environment, &to),
        };
    }
    match live {
        Some(id) => Ok(Some(id)),
        None => start_object(tx, event, target, None).map(Some),
    }
}

fn rename_object(
    tx: &Transaction<'_>,
    event: &ChangeEvent,
    object_id: i64,
    to: &ObjectRef,
    statement_id: i64,
) -> Result<(), DuckDbHistoryError> {
    if let Some(other) = live_object(tx, &event.environment, to)?
        && other != object_id
    {
        end_object(tx, other, event.run_time, statement_id)?;
    }
    let time = timestamp(event.run_time);
    close_name(tx, object_id, &time, statement_id)?;
    add_name(tx, object_id, to, &time, Some(statement_id))?;
    tx.execute(
        "UPDATE objects SET database = ?, name = ?, identifier = ? WHERE object_id = ?",
        params![to.schema, to.name, to.qualified_name(), object_id],
    )?;
    Ok(())
}

fn insert_statement(
    tx: &Transaction<'_>,
    event: &ChangeEvent,
    statement_id: i64,
    object_id: Option<i64>,
) -> Result<(), DuckDbHistoryError> {
    let target = event.target.as_ref();
    let database = target.and_then(|t| t.schema.clone());
    let name = target.map(|t| t.name.clone());
    let identifier = target.map(ObjectRef::qualified_name);
    let object_type = target.map(|t| t.object_type.as_str());
    let success = event.outcome.took_effect().then_some(true).or_else(|| {
        (event.outcome == vdp_vaultspeed_domain::ChangeOutcome::Failed).then_some(false)
    });
    let renamed_to = rename_target(event).map(|t| t.qualified_name());
    let rules = event.applied_rules.join(&LIST_SEP.to_string());
    let run_time = timestamp(event.run_time);
    let rows_affected = event.rows_affected;
    let error_code = event.error_code.map(i32::from);
    let duration_ms = event
        .duration_ms
        .map(|d| i64::try_from(d).unwrap_or(i64::MAX));
    let statement_index = i64::from(event.statement_index);
    let ledger_sequence = event
        .ledger_sequence
        .map(|s| i64::try_from(s).unwrap_or(i64::MAX));
    let operation_name = operation(&event.kind);
    let kind_label = event.kind.label();
    let formatted = format_sql(&event.effective_sql);
    let raw_fp = event.raw_fp.to_hex();
    let effective_fp = event.effective_fp.to_hex();
    let outcome = event.outcome.as_str();
    let column = event.column();
    let run_id = event.run_id.as_str();
    let separator = LIST_SEP.to_string();
    let values: Vec<&dyn ToSql> = vec![
        &database,
        &database,
        &name,
        &identifier,
        &event.raw_sql,
        &formatted,
        &event.environment,
        &raw_fp,
        &run_time,
        &run_time,
        &success,
        &rows_affected,
        &statement_id,
        &object_id,
        &object_type,
        &operation_name,
        &kind_label,
        &column,
        &renamed_to,
        &outcome,
        &event.decision,
        &error_code,
        &event.error_message,
        &duration_ms,
        &event.effective_sql,
        &raw_fp,
        &effective_fp,
        &rules,
        &rules,
        &separator,
        &event.environment,
        &run_id,
        &event.source,
        &event.job,
        &event.rel_path,
        &event.group_id,
        &event.phase,
        &statement_index,
        &ledger_sequence,
        &event.git_sha,
    ];
    tx.execute(
        "INSERT INTO statements (database, schema, name, identifier, raw_sql, formatted_sql, creation_time, \
         run_time, success, rows_affected, statement_id, object_id, object_type, operation, statement_kind, \
         column_name, renamed_to, outcome, decision, error_code, error_message, duration_ms, effective_sql, \
         raw_fp, effective_fp, applied_rules, environment, run_id, source, job, rel_path, group_id, phase, \
         statement_index, ledger_sequence, git_sha) \
         VALUES (?, ?, ?, ?, ?, ?, \
         COALESCE((SELECT min(run_time) FROM statements WHERE environment = ? AND raw_fp = ?), \
                  CAST(? AS TIMESTAMPTZ)), \
         CAST(? AS TIMESTAMPTZ), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
         CASE WHEN ? = '' THEN CAST([] AS VARCHAR[]) ELSE string_split(?, ?) END, \
         ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        values.as_slice(),
    )?;
    Ok(())
}
