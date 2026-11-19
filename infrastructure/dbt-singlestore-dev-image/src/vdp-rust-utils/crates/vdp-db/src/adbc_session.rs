//! [`SqlSession`] over SingleStore's ADBC driver (`libadbc_driver_singlestore.so`, the one dbt v2 uses).
//!
//! The driver is a shared library loaded at runtime ([`DriverSource`]); connection settings become its `mysql://`
//! URI the way dbt v2 builds it from a profile (`ssl_mode` → `tls`, `ssl_ca`/`ssl_cert`/`ssl_key`), with the user
//! and password passed as options, never in the URI. Parameters are bound as a one-row Arrow batch; results come
//! back as Arrow and are turned into [`SqlValue`]s.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use adbc_core::error::Error as AdbcError;
use adbc_core::options::{AdbcVersion, OptionDatabase, OptionValue};
use adbc_core::{Connection, Database, Driver, Statement};
use adbc_driver_manager::{ManagedConnection, ManagedDriver};
use arrow_array::{
    Array, ArrayRef, Float64Array, Int64Array, NullArray, RecordBatch, RecordBatchReader,
    StringArray, UInt64Array, cast::AsArray, types,
};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_schema::{DataType, Field, Schema};
use secrecy::ExposeSecret;
use vdp_sql_script::{RoutineAwareSplitter, StatementSplitter};

use crate::{ConnectionSettings, DbError, ResultSet, Row, SqlSession, SqlValue, TlsMode};

/// The driver's file name on Linux.
pub const DRIVER_FILE: &str = "libadbc_driver_singlestore.so";

/// Where the SingleStore ADBC driver is loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverSource {
    /// This file.
    File(PathBuf),
    /// The system library search path (`LD_LIBRARY_PATH`, the loader's defaults).
    System,
}

impl DriverSource {
    /// `explicit` when given; otherwise `lib/dbt/libadbc_driver_singlestore.so` in a directory above one of
    /// `executables` (where dbt v2 itself finds it: `/usr/local/libexec/dbt/dbt` → `/usr/local/lib/dbt/`,
    /// `/opt/dbt/target/release/dbt` → `/opt/dbt/lib/dbt/`); otherwise the system search path.
    pub fn locate(explicit: Option<&Path>, executables: &[PathBuf]) -> Self {
        if let Some(path) = explicit {
            return Self::File(path.to_owned());
        }
        for executable in executables {
            let executable = executable
                .canonicalize()
                .unwrap_or_else(|_| executable.clone());
            for dir in executable.ancestors().skip(1) {
                let candidate = dir.join("lib").join("dbt").join(DRIVER_FILE);
                if candidate.is_file() {
                    return Self::File(candidate);
                }
            }
        }
        Self::System
    }

    fn load(&self) -> Result<ManagedDriver, DbError> {
        let loaded = match self {
            Self::File(path) => {
                ManagedDriver::load_dynamic_from_filename(path, None, AdbcVersion::V110)
            }
            Self::System => ManagedDriver::load_dynamic_from_name(
                "adbc_driver_singlestore",
                None,
                AdbcVersion::V110,
            ),
        };
        loaded.map_err(|e| {
            DbError::client(format!(
                "cannot load the SingleStore ADBC driver ({self}): {}",
                e.message
            ))
        })
    }
}

impl fmt::Display for DriverSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => write!(f, "{}", path.display()),
            Self::System => write!(f, "{DRIVER_FILE} on the library search path"),
        }
    }
}

/// [`SqlSession`] over the SingleStore ADBC driver. One connection; statements run in order, so session state
/// (`SET ...`) carries over.
pub struct AdbcSession {
    connection: Mutex<ManagedConnection>,
    endpoint: String,
    driver: DriverSource,
}

/// The driver's URI and options for `settings` (dbt v2's mapping of a SingleStore profile).
pub(crate) fn database_options(
    settings: &ConnectionSettings,
) -> Vec<(OptionDatabase, OptionValue)> {
    let mut query: Vec<(&str, String)> = vec![("multiStatements", "true".into())];
    let has_ca = settings.ca_certificate.is_some();
    let tls = match settings.tls {
        TlsMode::Disabled => "false",
        TlsMode::Preferred => "preferred",
        TlsMode::SkipVerify => "skip-verify",
        TlsMode::VerifyCa if has_ca => "verify_ca",
        TlsMode::VerifyCa | TlsMode::VerifyFull => "true",
    };
    query.push(("tls", tls.into()));
    let timeout = settings.connect_timeout.as_secs().max(1);
    query.push(("timeout", format!("{timeout}s")));
    let database = settings.database.as_deref().map(encode).unwrap_or_default();
    let host = if settings.host.contains(':') && !settings.host.starts_with('[') {
        format!("[{}]", settings.host)
    } else {
        settings.host.clone()
    };
    let uri = format!(
        "mysql://{host}:{}/{database}?{}",
        settings.port,
        query
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    );
    let mut options = vec![
        (OptionDatabase::Uri, OptionValue::String(uri)),
        (
            OptionDatabase::Username,
            OptionValue::String(settings.user.clone()),
        ),
    ];
    if let Some(password) = &settings.password {
        options.push((
            OptionDatabase::Password,
            OptionValue::String(password.expose_secret().to_owned()),
        ));
    }
    let path = |p: &Path| OptionValue::String(p.display().to_string());
    if let Some(ca) = &settings.ca_certificate {
        options.push((OptionDatabase::Other("ssl_ca".into()), path(ca)));
    }
    if let Some(client) = &settings.client_certificate {
        options.push((
            OptionDatabase::Other("ssl_cert".into()),
            path(&client.certificate),
        ));
        options.push((OptionDatabase::Other("ssl_key".into()), path(&client.key)));
    }
    options
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

impl AdbcSession {
    /// Opens a session to `settings` through the driver loaded from `driver`.
    ///
    /// # Errors
    ///
    /// [`DbError`] without a code when the driver library cannot be loaded; [`DbError`] when the driver cannot create the
    /// database or connection (unreachable server, rejected credentials, bad TLS material), carrying the driver's vendor
    /// code when it reports one.
    pub fn connect(driver: &DriverSource, settings: &ConnectionSettings) -> Result<Self, DbError> {
        let mut loaded = driver.load()?;
        let database = loaded
            .new_database_with_opts(database_options(settings))
            .map_err(to_db_error)?;
        let connection = database.new_connection().map_err(to_db_error)?;
        tracing::debug!(endpoint = %settings.endpoint(), %driver, "connected to SingleStore (ADBC)");
        Ok(Self {
            connection: Mutex::new(connection),
            endpoint: settings.endpoint(),
            driver: driver.clone(),
        })
    }

    /// The driver this session was opened with.
    pub fn driver(&self) -> &DriverSource {
        &self.driver
    }

    fn with_connection<T>(
        &self,
        f: impl FnOnce(&mut ManagedConnection) -> Result<T, AdbcError>,
    ) -> Result<T, DbError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DbError::client("database session mutex poisoned"))?;
        f(&mut connection).map_err(to_db_error)
    }

    /// One statement: its result set (no columns for statements without one).
    fn run(&self, sql: &str, params: &[SqlValue]) -> Result<ResultSet, DbError> {
        let reader = self.with_connection(|connection| {
            let mut statement = connection.new_statement()?;
            statement.set_sql_query(sql)?;
            if !params.is_empty() {
                statement.bind(parameter_batch(params))?;
            }
            statement.execute()
        })?;
        read_result(reader)
    }
}

impl fmt::Debug for AdbcSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdbcSession")
            .field("endpoint", &self.endpoint)
            .field("driver", &self.driver)
            .finish_non_exhaustive()
    }
}

impl SqlSession for AdbcSession {
    fn execute(&self, sql: &str) -> Result<(), DbError> {
        self.with_connection(|connection| {
            let mut statement = connection.new_statement()?;
            statement.set_sql_query(sql)?;
            statement.execute_update().map(|_| ())
        })
    }

    fn execute_counted(&self, sql: &str) -> Result<Option<i64>, DbError> {
        self.with_connection(|connection| {
            let mut statement = connection.new_statement()?;
            statement.set_sql_query(sql)?;
            // ADBC reports "unknown" as a negative count (or none at all).
            statement
                .execute_update()
                .map(|rows| rows.filter(|n| *n >= 0))
        })
    }

    fn execute_with(&self, sql: &str, params: &[SqlValue]) -> Result<(), DbError> {
        self.with_connection(|connection| {
            let mut statement = connection.new_statement()?;
            statement.set_sql_query(sql)?;
            if !params.is_empty() {
                statement.bind(parameter_batch(params))?;
            }
            statement.execute_update().map(|_| ())
        })
    }

    fn query(&self, sql: &str, params: &[SqlValue]) -> Result<Vec<Row>, DbError> {
        Ok(self.query_table(sql, params)?.rows)
    }

    /// Without parameters the text may be a script: its statements run in order and the last result set with
    /// columns is returned, as a SQL client shows the final SELECT of a script.
    fn query_table(&self, sql: &str, params: &[SqlValue]) -> Result<ResultSet, DbError> {
        if !params.is_empty() {
            return self.run(sql, params);
        }
        let statements = RoutineAwareSplitter.split(sql);
        if statements.len() <= 1 {
            return self.run(statements.first().copied().unwrap_or(sql), &[]);
        }
        let mut last = ResultSet::default();
        for statement in statements {
            let result = self.run(statement, &[])?;
            if !result.columns.is_empty() {
                last = result;
            }
        }
        Ok(last)
    }
}

/// The parameters as one Arrow row, column `i` for placeholder `i`.
fn parameter_batch(params: &[SqlValue]) -> RecordBatch {
    let (fields, columns): (Vec<Field>, Vec<ArrayRef>) = params
        .iter()
        .enumerate()
        .map(|(i, value)| {
            let name = i.to_string();
            match value {
                SqlValue::Null => (
                    Field::new(name, DataType::Null, true),
                    Arc::new(NullArray::new(1)) as ArrayRef,
                ),
                SqlValue::Int(v) => (
                    Field::new(name, DataType::Int64, true),
                    Arc::new(Int64Array::from(vec![*v])) as ArrayRef,
                ),
                SqlValue::UInt(v) => (
                    Field::new(name, DataType::UInt64, true),
                    Arc::new(UInt64Array::from(vec![*v])) as ArrayRef,
                ),
                SqlValue::Float(v) => (
                    Field::new(name, DataType::Float64, true),
                    Arc::new(Float64Array::from(vec![*v])) as ArrayRef,
                ),
                SqlValue::Text(v) => (
                    Field::new(name, DataType::Utf8, true),
                    Arc::new(StringArray::from(vec![v.as_str()])) as ArrayRef,
                ),
            }
        })
        .unzip();
    RecordBatch::try_new(Arc::new(Schema::new(fields)), columns)
        .expect("one value per column, one row each")
}

fn read_result(reader: Box<dyn RecordBatchReader + Send>) -> Result<ResultSet, DbError> {
    let columns: Vec<String> = reader
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| DbError::client(format!("reading the result: {e}")))?;
        for row in 0..batch.num_rows() {
            rows.push(
                batch
                    .columns()
                    .iter()
                    .map(|column| value(column.as_ref(), row))
                    .collect::<Result<Row, DbError>>()?,
            );
        }
    }
    Ok(ResultSet { columns, rows })
}

/// SQL-style text for values without a [`SqlValue`] of their own (`2026-11-09 16:12:32.801957`, `12.50`).
fn format_options() -> FormatOptions<'static> {
    FormatOptions::default()
        .with_timestamp_format(Some("%Y-%m-%d %H:%M:%S%.f"))
        .with_timestamp_tz_format(Some("%Y-%m-%d %H:%M:%S%.f"))
        .with_datetime_format(Some("%Y-%m-%d %H:%M:%S%.f"))
}

fn value(column: &dyn Array, row: usize) -> Result<SqlValue, DbError> {
    if column.is_null(row) {
        return Ok(SqlValue::Null);
    }
    Ok(match column.data_type() {
        DataType::Null => SqlValue::Null,
        DataType::Boolean => SqlValue::Int(i64::from(column.as_boolean().value(row))),
        DataType::Int8 => SqlValue::Int(column.as_primitive::<types::Int8Type>().value(row).into()),
        DataType::Int16 => {
            SqlValue::Int(column.as_primitive::<types::Int16Type>().value(row).into())
        }
        DataType::Int32 => {
            SqlValue::Int(column.as_primitive::<types::Int32Type>().value(row).into())
        }
        DataType::Int64 => SqlValue::Int(column.as_primitive::<types::Int64Type>().value(row)),
        DataType::UInt8 => {
            SqlValue::UInt(column.as_primitive::<types::UInt8Type>().value(row).into())
        }
        DataType::UInt16 => {
            SqlValue::UInt(column.as_primitive::<types::UInt16Type>().value(row).into())
        }
        DataType::UInt32 => {
            SqlValue::UInt(column.as_primitive::<types::UInt32Type>().value(row).into())
        }
        DataType::UInt64 => SqlValue::UInt(column.as_primitive::<types::UInt64Type>().value(row)),
        DataType::Float32 => SqlValue::Float(
            column
                .as_primitive::<types::Float32Type>()
                .value(row)
                .into(),
        ),
        DataType::Float64 => {
            SqlValue::Float(column.as_primitive::<types::Float64Type>().value(row))
        }
        DataType::Utf8 => SqlValue::Text(column.as_string::<i32>().value(row).to_owned()),
        DataType::LargeUtf8 => SqlValue::Text(column.as_string::<i64>().value(row).to_owned()),
        DataType::Utf8View => SqlValue::Text(column.as_string_view().value(row).to_owned()),
        DataType::Binary => SqlValue::Text(
            String::from_utf8_lossy(column.as_binary::<i32>().value(row)).into_owned(),
        ),
        DataType::LargeBinary => SqlValue::Text(
            String::from_utf8_lossy(column.as_binary::<i64>().value(row)).into_owned(),
        ),
        _ => {
            let options = format_options();
            let formatter = ArrayFormatter::try_new(column, &options).map_err(|e| {
                DbError::client(format!("cannot show a {} value: {e}", column.data_type()))
            })?;
            SqlValue::Text(formatter.value(row).to_string())
        }
    })
}

/// The server's error number (the driver's vendor code) and its message without the driver's prefixes
/// (`[singlestore] [SingleStore] failed to execute statement: Error 1060 (42S21): Duplicate column name 'x'`).
fn to_db_error(error: AdbcError) -> DbError {
    let code = u16::try_from(error.vendor_code).ok().filter(|c| *c > 0);
    let message = match code.and_then(|c| {
        let marker = format!("Error {c} (");
        error
            .message
            .find(&marker)
            .and_then(|at| error.message[at..].find("): ").map(|end| at + end + 3))
    }) {
        Some(start) => error.message[start..].to_owned(),
        None => error.message,
    };
    DbError { code, message }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use secrecy::SecretString;

    use super::*;
    use crate::ClientCertificate;

    fn settings(tls: TlsMode, ca: bool) -> ConnectionSettings {
        let mut settings = ConnectionSettings::new("singlestore.example", 3307, "dbt user");
        settings.password = Some(SecretString::from("p@ss:wörd"));
        settings.database = Some("edw dv".into());
        settings.tls = tls;
        settings.ca_certificate = ca.then(|| PathBuf::from("/assets/ca.cert"));
        settings.connect_timeout = Duration::from_secs(30);
        settings
    }

    fn uri(options: &[(OptionDatabase, OptionValue)]) -> String {
        match &options[0] {
            (OptionDatabase::Uri, OptionValue::String(uri)) => uri.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn options_follow_dbt_v2_and_keep_secrets_out_of_the_uri() {
        let options = database_options(&settings(TlsMode::VerifyCa, true));
        let uri = uri(&options);
        assert_eq!(
            uri,
            "mysql://singlestore.example:3307/edw%20dv?multiStatements=true&tls=verify_ca&timeout=30s"
        );
        assert!(!uri.contains("p@ss") && !uri.contains("dbt"));
        let others: Vec<String> = options[1..]
            .iter()
            .map(|(k, v)| format!("{k:?}={v:?}"))
            .collect();
        assert_eq!(others.len(), 3, "{others:?}");
        assert!(others[0].contains("Username") && others[0].contains("dbt user"));
        assert!(others[1].contains("Password") && others[1].contains("wörd"));
        assert!(others[2].contains("ssl_ca") && others[2].contains("/assets/ca.cert"));
    }

    #[test]
    fn tls_modes() {
        let tls = |mode, ca| {
            let uri = uri(&database_options(&settings(mode, ca)));
            uri.split("tls=")
                .nth(1)
                .unwrap()
                .split('&')
                .next()
                .unwrap()
                .to_owned()
        };
        assert_eq!(tls(TlsMode::Disabled, false), "false");
        assert_eq!(tls(TlsMode::Preferred, false), "preferred");
        assert_eq!(tls(TlsMode::SkipVerify, false), "skip-verify");
        assert_eq!(tls(TlsMode::VerifyCa, false), "true");
        assert_eq!(tls(TlsMode::VerifyFull, true), "true");
        let mut client = settings(TlsMode::VerifyFull, false);
        client.client_certificate = Some(ClientCertificate {
            certificate: "/c.pem".into(),
            key: "/k.pem".into(),
        });
        let keys: Vec<String> = database_options(&client)
            .iter()
            .map(|(k, _)| format!("{k:?}"))
            .collect();
        assert!(
            keys.iter().any(|k| k.contains("ssl_cert"))
                && keys.iter().any(|k| k.contains("ssl_key"))
        );
    }

    #[test]
    fn parameters_and_results_round_trip_through_arrow() {
        let batch = parameter_batch(&[
            SqlValue::Null,
            SqlValue::Int(-3),
            SqlValue::UInt(7),
            SqlValue::Float(1.5),
            SqlValue::Text("één".into()),
        ]);
        assert_eq!(batch.num_rows(), 1);
        let back: Row = batch
            .columns()
            .iter()
            .map(|c| value(c.as_ref(), 0).unwrap())
            .collect();
        assert_eq!(
            back,
            [
                SqlValue::Null,
                SqlValue::Int(-3),
                SqlValue::UInt(7),
                SqlValue::Float(1.5),
                SqlValue::Text("één".into())
            ]
        );
    }

    #[test]
    fn server_errors_keep_their_code_and_message() {
        let mut error = AdbcError::with_message_and_status(
            "[singlestore] [SingleStore] failed to execute statement: Error 1060 (42S21): Duplicate column name 'x'",
            adbc_core::error::Status::InvalidArguments,
        );
        error.vendor_code = 1060;
        assert_eq!(
            to_db_error(error),
            DbError {
                code: Some(1060),
                message: "Duplicate column name 'x'".into()
            }
        );
        let client =
            AdbcError::with_message_and_status("no connection", adbc_core::error::Status::IO);
        assert_eq!(to_db_error(client).code, None);
    }

    #[test]
    fn the_driver_is_found_above_an_executable() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib/dbt");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join(DRIVER_FILE), b"").unwrap();
        let exe = dir.path().join("libexec/dbt/dbt");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let found = DriverSource::locate(None, &[PathBuf::from("/nonexistent/bin/vdp"), exe]);
        assert_eq!(
            found,
            DriverSource::File(lib.join(DRIVER_FILE).canonicalize().unwrap())
        );
        assert_eq!(
            DriverSource::locate(Some(Path::new("/x/driver.so")), &[]),
            DriverSource::File("/x/driver.so".into())
        );
        assert_eq!(DriverSource::locate(None, &[]), DriverSource::System);
    }
}
