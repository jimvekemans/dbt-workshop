//! The database port shared by every module that talks to SingleStore.
//!
//! [`SqlSession`] is intentionally tiny so that tests can fake it in a few lines and so that no other crate
//! depends on a concrete driver. The one implementation, [`AdbcSession`], talks to SingleStore through its ADBC
//! driver (`libadbc_driver_singlestore.so`, the one dbt v2 uses), loaded at runtime. Connection settings follow the
//! dbt v2 profile vocabulary (TLS modes, CA and client certificates); results are [`ResultSet`]s with column names.

mod adbc_session;
mod csv_out;
mod session;
mod settings;

pub use adbc_session::{AdbcSession, DRIVER_FILE, DriverSource};
pub use csv_out::write_csv;
pub use session::{DbError, ResultSet, Row, SqlSession, SqlValue};
pub use settings::{ClientCertificate, ConnectionSettings, TlsMode};
