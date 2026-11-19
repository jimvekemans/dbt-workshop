//! `vdp sql ...`

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use vdp_core::{Context, Outcome};
use vdp_db::SqlSession;

use crate::args::SqlCommand;

fn csv_path(dir: &Path, file: &Path) -> PathBuf {
    let stem = file
        .file_stem()
        .map_or_else(|| "result".into(), |s| s.to_string_lossy().into_owned());
    dir.join(format!("{stem}.csv"))
}

fn format_dry_run(file: &Path, bytes: usize, csv_dir: Option<&Path>) -> String {
    let mut out = format!(
        "== {}\nwould run {} ({bytes} bytes)\n",
        file.display(),
        file.display()
    );
    if let Some(dir) = csv_dir {
        let _ = writeln!(out, "would save {}", csv_path(dir, file).display());
    }
    out
}

pub(super) fn sql(ctx: &Context, command: &SqlCommand) -> Result<Outcome> {
    match command {
        SqlCommand::Run {
            files,
            csv_dir,
            limit,
            dry_run: false,
        } => {
            let session = ctx.service::<Arc<dyn SqlSession>>()?;
            for file in files {
                println!("== {}", file.display());
                let result = vdp_sql::run_file(session.as_ref(), file)?;
                print!("{}", vdp_sql::render(&result, *limit));
                if let Some(dir) = csv_dir {
                    let path = csv_path(dir, file);
                    vdp_sql::save_csv(&result, &path)?;
                    println!("saved {}", path.display());
                }
            }
        }
        SqlCommand::Run {
            files,
            csv_dir,
            limit: _,
            dry_run: true,
        } => {
            for file in files {
                let sql = vdp_sql::read_sql_file(file)?;
                print!("{}", format_dry_run(file, sql.len(), csv_dir.as_deref()));
            }
        }
        SqlCommand::Export { table, out } => {
            let session = ctx.service::<Arc<dyn SqlSession>>()?;
            let result = vdp_sql::export_table(session.as_ref(), table)?;
            let path = out.clone().unwrap_or_else(|| {
                PathBuf::from("data").join(format!("{}.csv", table.replace('.', "_")))
            });
            vdp_sql::save_csv(&result, &path)?;
            println!("saved {} rows to {}", result.rows.len(), path.display());
        }
    }
    Ok(Outcome::Success)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use vdp_core::Config;
    use vdp_db::{DbError, ResultSet, Row, SqlValue};

    use super::*;

    #[derive(Debug, Default)]
    struct FakeSqlSession {
        calls: Mutex<Vec<String>>,
    }

    impl FakeSqlSession {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl SqlSession for FakeSqlSession {
        fn execute(&self, sql: &str) -> Result<(), DbError> {
            self.calls.lock().unwrap().push(sql.to_owned());
            Ok(())
        }

        fn execute_with(&self, sql: &str, _params: &[SqlValue]) -> Result<(), DbError> {
            self.calls.lock().unwrap().push(sql.to_owned());
            Ok(())
        }

        fn query(&self, sql: &str, _params: &[SqlValue]) -> Result<Vec<Row>, DbError> {
            self.calls.lock().unwrap().push(sql.to_owned());
            Ok(Vec::new())
        }

        fn query_table(&self, sql: &str, _params: &[SqlValue]) -> Result<ResultSet, DbError> {
            self.calls.lock().unwrap().push(sql.to_owned());
            Ok(ResultSet {
                columns: vec!["n".into()],
                rows: vec![vec![SqlValue::Int(1)]],
            })
        }
    }

    #[test]
    fn sql_run_dry_run_skips_session_and_csv_writes() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("query.sql");
        std::fs::write(&script, "SELECT 1;\n").unwrap();
        let csv_out = dir.path().join("csv_out");

        let bare_ctx = Context::builder(Config::default()).build();
        let outcome = super::sql(
            &bare_ctx,
            &SqlCommand::Run {
                files: vec![script.clone()],
                csv_dir: Some(csv_out.clone()),
                limit: 50,
                dry_run: true,
            },
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Success);
        assert!(!csv_out.exists(), "dry-run must not create csv_dir");
        assert_eq!(
            format_dry_run(&script, 10, Some(&csv_out)),
            format!(
                "== {}\nwould run {} (10 bytes)\nwould save {}\n",
                script.display(),
                script.display(),
                csv_out.join("query.csv").display()
            )
        );

        let session = Arc::new(FakeSqlSession::default());
        let recorded_ctx = Context::builder(Config::default())
            .provide_value(session.clone() as Arc<dyn SqlSession>)
            .build();
        let dry = super::sql(
            &recorded_ctx,
            &SqlCommand::Run {
                files: vec![script.clone()],
                csv_dir: Some(csv_out.clone()),
                limit: 50,
                dry_run: true,
            },
        )
        .unwrap();
        assert_eq!(dry, Outcome::Success);
        assert_eq!(session.calls(), Vec::<String>::new());
        assert!(!csv_out.exists());

        let live = super::sql(
            &recorded_ctx,
            &SqlCommand::Run {
                files: vec![script],
                csv_dir: Some(csv_out.clone()),
                limit: 50,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(live, Outcome::Success);
        assert_eq!(session.calls(), ["SELECT 1;\n"]);
        assert!(csv_out.join("query.csv").is_file());
    }
}
