//! The database port shared by every module that talks to SingleStore.
//!
//! A building block. [`SqlSession`] is intentionally tiny so that tests can fake it in a few lines and so that no other
//! crate depends on a concrete driver. The one implementation, [`AdbcSession`], talks to SingleStore through its ADBC
//! driver (`libadbc_driver_singlestore.so`, the one dbt v2 uses), loaded at runtime.
//!
//! # Responsibilities
//!
//! - Define the [`SqlSession`] port, with its value ([`SqlValue`]), row ([`Row`]) and result ([`ResultSet`], with column
//!   names) types and the [`DbError`] (with the server error code when the server produced it).
//! - Implement it over ADBC ([`AdbcSession`]), locating the driver library ([`DriverSource`]).
//! - Read connection settings in the dbt v2 profile vocabulary (TLS modes, CA and client certificates), from a map or
//!   from secrets ([`ConnectionSettings`]).
//! - Write a result set as RFC 4180 CSV ([`write_csv`]).
//!
//! # Example
//!
//! ```no_run
//! use vdp_db::{AdbcSession, ConnectionSettings, DriverSource, SqlSession, TlsMode};
//!
//! let settings = ConnectionSettings::new("127.0.0.1", 3306, "root");
//! let driver = DriverSource::locate(None, &[]);
//! let session = AdbcSession::connect(&driver, &settings)?;
//! let table = session.query_table("SELECT 1 AS one", &[])?;
//! assert_eq!(table.columns, ["one"]);
//! # let _ = TlsMode::Disabled;
//! # Ok::<(), vdp_db::DbError>(())
//! ```

mod adbc_session;
mod csv_out;
mod session;
mod settings;

pub use adbc_session::{AdbcSession, DRIVER_FILE, DriverSource};
pub use csv_out::write_csv;
pub use session::{DbError, ResultSet, Row, SqlSession, SqlValue};
pub use settings::{
    ClientCertificate, ConnectionSettings, ConnectionSettingsBuilder, SettingsError, TlsMode,
};
