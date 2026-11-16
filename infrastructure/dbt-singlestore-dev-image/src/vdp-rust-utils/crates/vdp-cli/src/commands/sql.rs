//! `vdp sql ...`

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use vdp_core::{Context, Outcome};
use vdp_db::SqlSession;

use crate::args::SqlCommand;

pub(super) fn sql(ctx: &Context, command: &SqlCommand) -> Result<Outcome> {
    let session = ctx.service::<Arc<dyn SqlSession>>()?;
    match command {
        SqlCommand::Run {
            files,
            csv_dir,
            limit,
        } => {
            for file in files {
                println!("== {}", file.display());
                let result = vdp_sql::run_file(session.as_ref(), file)?;
                print!("{}", vdp_sql::render(&result, *limit));
                if let Some(dir) = csv_dir {
                    let stem = file
                        .file_stem()
                        .map_or_else(|| "result".into(), |s| s.to_string_lossy().into_owned());
                    let path = dir.join(format!("{stem}.csv"));
                    vdp_sql::save_csv(&result, &path)?;
                    println!("saved {}", path.display());
                }
            }
        }
        SqlCommand::Export { table, out } => {
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
