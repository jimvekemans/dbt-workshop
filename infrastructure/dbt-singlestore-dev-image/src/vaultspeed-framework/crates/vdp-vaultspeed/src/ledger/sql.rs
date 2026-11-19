use std::sync::Arc;

use chrono::{NaiveDateTime, SubsecRound, TimeZone, Utc};
use vdp_db::{Row, SqlSession, SqlValue};
use vdp_vaultspeed_domain::{Fingerprint, LedgerEntry, RunId};

use crate::ledger::{LedgerError, LedgerReader, LedgerWriter, MemoryLedger};

/// Location of the ledger table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerTable {
    schema: String,
    table: String,
}

impl LedgerTable {
    /// Identifiers are restricted to `[A-Za-z0-9_]` because they are interpolated into DDL.
    pub fn new(schema: &str, table: &str) -> Result<Self, LedgerError> {
        let valid =
            |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !valid(schema) || !valid(table) {
            return Err(LedgerError::MalformedRow(format!(
                "invalid ledger table identifier `{schema}`.`{table}`"
            )));
        }
        Ok(Self {
            schema: schema.to_owned(),
            table: table.to_owned(),
        })
    }

    fn qualified(&self) -> String {
        format!("`{}`.`{}`", self.schema, self.table)
    }
}

const COLUMNS: &str = "`sequence`, `run_id`, `recorded_at`, `source`, `job`, `rel_path`, \
    `object_key`, `statement_kind`, `raw_fp`, `effective_fp`, `outcome`, `detail`, `git_sha`, \
    `renamed_to`";
/// What a table without the `renamed_to` column (created before renames were tracked) is read as.
const COLUMNS_WITHOUT_RENAMED_TO: &str = "`sequence`, `run_id`, `recorded_at`, `source`, `job`, \
    `rel_path`, `object_key`, `statement_kind`, `raw_fp`, `effective_fp`, `outcome`, `detail`, \
    `git_sha`, NULL";
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.6f";

/// Ledger stored in the environment's own database, the source of truth: the record of
/// what ran lives next to the schema it describes. All rows are loaded at open time;
/// lookups are served from memory and writes go to the table first.
#[derive(Debug)]
pub struct SqlLedger {
    session: Arc<dyn SqlSession>,
    table: LedgerTable,
    memory: MemoryLedger,
}

impl SqlLedger {
    /// Creates the table when missing (the schema must exist) and loads all rows.
    pub fn open(session: Arc<dyn SqlSession>, table: LedgerTable) -> Result<Self, LedgerError> {
        Self::open_in(session, table, "")
    }

    /// Like [`SqlLedger::open`]; unqualified object keys of older rows are qualified with `database`, the
    /// environment's default, in memory only (see [`MemoryLedger::from_entries_in`]).
    pub fn open_in(
        session: Arc<dyn SqlSession>,
        table: LedgerTable,
        database: &str,
    ) -> Result<Self, LedgerError> {
        session.execute(&format!(
            "CREATE ROWSTORE TABLE IF NOT EXISTS {} (\
               `sequence` BIGINT UNSIGNED NOT NULL, \
               `run_id` VARCHAR(64) NOT NULL, \
               `recorded_at` DATETIME(6) NOT NULL, \
               `source` VARCHAR(255) NOT NULL, \
               `job` VARCHAR(512) NOT NULL, \
               `rel_path` VARCHAR(1024) NOT NULL, \
               `object_key` VARCHAR(1024) NULL, \
               `statement_kind` VARCHAR(64) NOT NULL, \
               `raw_fp` CHAR(64) NULL, \
               `effective_fp` CHAR(64) NOT NULL, \
               `outcome` VARCHAR(16) NOT NULL, \
               `detail` LONGTEXT NULL, \
               `git_sha` VARCHAR(64) NULL, \
               `renamed_to` VARCHAR(1024) NULL, \
               PRIMARY KEY (`sequence`))",
            table.qualified()
        ))?;
        if !has_renamed_to(session.as_ref(), &table)? {
            session.execute(&format!(
                "ALTER TABLE {} ADD COLUMN `renamed_to` VARCHAR(1024) NULL",
                table.qualified()
            ))?;
            tracing::info!(table = %table.qualified(), "added renamed_to column to the ledger table");
        }
        let entries = load_entries(session.as_ref(), &table, true)?;
        tracing::info!(entries = entries.len(), table = %table.qualified(), "loaded ledger");
        Ok(Self {
            session,
            table,
            memory: MemoryLedger::from_entries_in(database, entries),
        })
    }

    /// Reads all rows without creating the table (preview runs). A missing table is an
    /// empty ledger.
    pub fn snapshot(
        session: &dyn SqlSession,
        table: &LedgerTable,
    ) -> Result<MemoryLedger, LedgerError> {
        Self::snapshot_in(session, table, "")
    }

    /// Like [`SqlLedger::snapshot`], qualifying unqualified object keys with `database`. A table without
    /// the `renamed_to` column is read as if the column were empty; a snapshot never alters the table.
    pub fn snapshot_in(
        session: &dyn SqlSession,
        table: &LedgerTable,
        database: &str,
    ) -> Result<MemoryLedger, LedgerError> {
        let loaded = has_renamed_to(session, table)
            .and_then(|present| load_entries(session, table, present));
        match loaded {
            Ok(entries) => Ok(MemoryLedger::from_entries_in(database, entries)),
            Err(LedgerError::Database(error)) if error.code == Some(NO_SUCH_TABLE) => {
                Ok(MemoryLedger::new())
            }
            Err(error) => Err(error),
        }
    }

    fn insert(&self, entry: &LedgerEntry) -> Result<(), LedgerError> {
        let params = vec![
            SqlValue::UInt(entry.sequence),
            entry.run_id.as_str().into(),
            entry
                .recorded_at
                .format(TIMESTAMP_FORMAT)
                .to_string()
                .into(),
            entry.source.clone().into(),
            entry.job.clone().into(),
            entry.rel_path.clone().into(),
            entry.object_key.clone().into(),
            entry.statement_kind.clone().into(),
            entry.raw_fp.map(|fp| fp.to_hex()).into(),
            entry.effective_fp.to_hex().into(),
            entry.outcome.as_str().into(),
            entry.detail.clone().into(),
            entry.git_sha.clone().into(),
            entry.renamed_to.clone().into(),
        ];
        let placeholders = vec!["?"; params.len()].join(", ");
        self.session.execute_with(
            &format!(
                "INSERT INTO {} ({COLUMNS}) VALUES ({placeholders})",
                self.table.qualified()
            ),
            &params,
        )?;
        Ok(())
    }
}

const NO_SUCH_TABLE: u16 = 1146;

/// Whether the ledger table has the `renamed_to` column. A missing table has none: the caller either
/// creates it (with the column) or reads nothing from it.
fn has_renamed_to(session: &dyn SqlSession, table: &LedgerTable) -> Result<bool, LedgerError> {
    let rows = session.query(
        "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND COLUMN_NAME = ?",
        &[
            table.schema.as_str().into(),
            table.table.as_str().into(),
            "renamed_to".into(),
        ],
    )?;
    Ok(!rows.is_empty())
}

fn load_entries(
    session: &dyn SqlSession,
    table: &LedgerTable,
    with_renamed_to: bool,
) -> Result<Vec<LedgerEntry>, LedgerError> {
    let columns = if with_renamed_to {
        COLUMNS
    } else {
        COLUMNS_WITHOUT_RENAMED_TO
    };
    let sql = format!(
        "SELECT {columns} FROM {} ORDER BY `sequence`",
        table.qualified()
    );
    session
        .query(&sql, &[])?
        .iter()
        .map(entry_from_row)
        .collect()
}

fn entry_from_row(row: &Row) -> Result<LedgerEntry, LedgerError> {
    let malformed = |what: &str| LedgerError::MalformedRow(what.to_owned());
    let text = |i: usize| row.get(i).and_then(SqlValue::as_text).map(str::to_owned);
    let required = |i: usize, name: &str| text(i).ok_or_else(|| malformed(name));
    let fingerprint = |s: String| {
        s.parse::<Fingerprint>()
            .map_err(|e| malformed(&e.to_string()))
    };

    let recorded_at_text = required(2, "recorded_at")?;
    let recorded_at = NaiveDateTime::parse_from_str(&recorded_at_text, TIMESTAMP_FORMAT)
        .or_else(|_| NaiveDateTime::parse_from_str(&recorded_at_text, "%Y-%m-%d %H:%M:%S"))
        .map_err(|e| malformed(&format!("recorded_at: {e}")))?;

    Ok(LedgerEntry {
        sequence: row
            .first()
            .and_then(SqlValue::as_u64)
            .ok_or_else(|| malformed("sequence"))?,
        run_id: RunId::from_string(required(1, "run_id")?),
        recorded_at: Utc.from_utc_datetime(&recorded_at),
        source: required(3, "source")?,
        job: required(4, "job")?,
        rel_path: required(5, "rel_path")?,
        object_key: text(6),
        statement_kind: required(7, "statement_kind")?,
        raw_fp: text(8).map(fingerprint).transpose()?,
        effective_fp: fingerprint(required(9, "effective_fp")?)?,
        outcome: required(10, "outcome")?
            .parse()
            .map_err(|e: vdp_vaultspeed_domain::DomainError| malformed(&e.to_string()))?,
        detail: text(11),
        git_sha: text(12),
        renamed_to: text(13),
    })
}

impl LedgerReader for SqlLedger {
    fn find_applied(
        &self,
        raw: Option<&Fingerprint>,
        effective: &Fingerprint,
    ) -> Result<Option<LedgerEntry>, LedgerError> {
        self.memory.find_applied(raw, effective)
    }

    fn latest_for_object(&self, object_key: &str) -> Result<Option<LedgerEntry>, LedgerError> {
        self.memory.latest_for_object(object_key)
    }

    fn object_history(&self, object_key: &str) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.memory.object_history(object_key)
    }

    fn last_sequence(&self) -> Result<u64, LedgerError> {
        self.memory.last_sequence()
    }

    fn entries(&self) -> Result<Vec<LedgerEntry>, LedgerError> {
        self.memory.entries()
    }
}

impl LedgerWriter for SqlLedger {
    /// The row is written before the in-memory index is updated, so a failed insert
    /// never makes a change look applied.
    fn record(&self, mut entry: LedgerEntry) -> Result<LedgerEntry, LedgerError> {
        entry.sequence = self.memory.last_sequence()? + 1;
        // DATETIME(6) keeps microseconds; truncate so memory and table agree.
        entry.recorded_at = entry.recorded_at.trunc_subsecs(6);
        self.insert(&entry)?;
        crate::ledger::LedgerSink::append(&self.memory, &entry)?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use vdp_db::DbError;
    use vdp_vaultspeed_domain::LedgerOutcome;

    use super::*;
    use crate::ledger::memory::tests::entry;

    /// Records statements and replays inserted rows on SELECT.
    #[derive(Debug, Default)]
    struct FakeSession {
        executed: Mutex<Vec<String>>,
        rows: Mutex<Vec<Row>>,
        fail_inserts: bool,
        /// Whether `information_schema.COLUMNS` shows the `renamed_to` column.
        has_renamed_to: bool,
        selects: Mutex<Vec<String>>,
    }

    impl SqlSession for FakeSession {
        fn execute(&self, sql: &str) -> Result<(), DbError> {
            self.executed.lock().unwrap().push(sql.to_owned());
            Ok(())
        }

        fn execute_with(&self, sql: &str, params: &[SqlValue]) -> Result<(), DbError> {
            if self.fail_inserts {
                return Err(DbError {
                    code: Some(1146),
                    message: "no table".into(),
                });
            }
            self.executed.lock().unwrap().push(sql.to_owned());
            self.rows.lock().unwrap().push(params.to_vec());
            Ok(())
        }

        fn query(&self, sql: &str, params: &[SqlValue]) -> Result<Vec<Row>, DbError> {
            if sql.contains("information_schema.COLUMNS") {
                assert_eq!(params.len(), 3, "the lookup binds its parameters");
                return Ok(if self.has_renamed_to {
                    vec![vec!["renamed_to".into()]]
                } else {
                    Vec::new()
                });
            }
            self.selects.lock().unwrap().push(sql.to_owned());
            Ok(self.rows.lock().unwrap().clone())
        }
    }

    fn session(has_renamed_to: bool) -> Arc<FakeSession> {
        Arc::new(FakeSession {
            has_renamed_to,
            ..FakeSession::default()
        })
    }

    fn table() -> LedgerTable {
        LedgerTable::new("edw_meta", "vsaf_ledger").unwrap()
    }

    #[test]
    fn creates_table_and_round_trips_rows() {
        let session = session(true);
        let ledger = SqlLedger::open(session.clone(), table()).unwrap();
        assert!(
            session.executed.lock().unwrap()[0]
                .starts_with("CREATE ROWSTORE TABLE IF NOT EXISTS `edw_meta`.`vsaf_ledger`")
        );
        ledger
            .record(entry(Some("a"), "a", LedgerOutcome::Applied))
            .unwrap();
        let mut moved = entry(Some("m"), "m", LedgerOutcome::Applied);
        moved.statement_kind = "ALTER TABLE RENAME".into();
        moved.renamed_to = Some("relation:s.u".into());
        ledger.record(moved).unwrap();
        ledger
            .record(entry(None, "b", LedgerOutcome::Imported))
            .unwrap();

        let reopened = SqlLedger::open(session, table()).unwrap();
        assert_eq!(reopened.entries().unwrap(), ledger.entries().unwrap());
        assert_eq!(reopened.last_sequence().unwrap(), 3);
        assert_eq!(
            reopened.entries().unwrap()[1].renamed_to.as_deref(),
            Some("relation:s.u")
        );
    }

    #[test]
    fn failed_insert_does_not_mark_applied() {
        let session = Arc::new(FakeSession {
            fail_inserts: true,
            has_renamed_to: true,
            ..FakeSession::default()
        });
        let ledger = SqlLedger::open(session, table()).unwrap();
        assert!(
            ledger
                .record(entry(Some("a"), "a", LedgerOutcome::Applied))
                .is_err()
        );
        assert!(
            ledger
                .find_applied(Some(&Fingerprint::of("a")), &Fingerprint::of("a"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn snapshot_is_read_only_and_tolerates_a_missing_table() {
        #[derive(Debug)]
        struct NoTable;
        impl SqlSession for NoTable {
            fn execute(&self, _: &str) -> Result<(), DbError> {
                panic!("a snapshot must not execute statements")
            }
            fn execute_with(&self, _: &str, _: &[SqlValue]) -> Result<(), DbError> {
                panic!("a snapshot must not execute statements")
            }
            fn query(&self, _: &str, _: &[SqlValue]) -> Result<Vec<Row>, DbError> {
                Err(DbError {
                    code: Some(1146),
                    message: "Table doesn't exist".into(),
                })
            }
        }
        let ledger = SqlLedger::snapshot(&NoTable, &table()).unwrap();
        assert_eq!(ledger.last_sequence().unwrap(), 0);
    }

    #[test]
    fn rejects_unsafe_identifiers() {
        assert!(LedgerTable::new("edw`; DROP", "x").is_err());
    }

    #[test]
    fn an_existing_table_gets_the_renamed_to_column() {
        let old = session(false);
        SqlLedger::open(old.clone(), table()).unwrap();
        let executed = old.executed.lock().unwrap();
        assert!(
            executed
                .iter()
                .any(|sql| sql.contains("ADD COLUMN `renamed_to` VARCHAR(1024) NULL")),
            "{executed:?}"
        );

        let current = session(true);
        SqlLedger::open(current.clone(), table()).unwrap();
        assert!(
            !current
                .executed
                .lock()
                .unwrap()
                .iter()
                .any(|sql| sql.starts_with("ALTER"))
        );
    }

    #[test]
    fn a_snapshot_reads_a_table_without_the_column() {
        let old = session(false);
        SqlLedger::snapshot(old.as_ref(), &table()).unwrap();
        assert!(old.executed.lock().unwrap().is_empty());
        assert!(old.selects.lock().unwrap()[0].contains("`git_sha`, NULL"));
    }
}
