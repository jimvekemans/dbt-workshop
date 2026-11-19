//! `vdp sources ...`

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use vdp_core::{BatchOutcome, Context, Outcome};
use vdp_db::SqlSession;
use vdp_dbt::artifacts::Manifest;
use vdp_dbt::{DbtProject, missing_sources};
use vdp_sources::rewrite::{RefreshOptions, Refreshed, TrimOptions, Trimmed};
use vdp_sources::{AddFailure, AddOptions, AddRequest, AddSuccess, SourcesSettings, SqlCatalog};

use super::common::{parse, print_dbt_errors, relative};
use crate::args::SourcesCommand;

pub(super) fn run(ctx: &Context, command: &SourcesCommand) -> Result<Outcome> {
    let settings: SourcesSettings = ctx.config().section(SourcesSettings::SECTION)?;
    match command {
        SourcesCommand::Add {
            tables,
            missing,
            dry_run,
        } => add(ctx, &settings, tables, *missing, *dry_run),
        SourcesCommand::Rewrite {
            databases,
            no_trim,
            dry_run,
        } => rewrite(
            ctx,
            &settings,
            databases.as_deref().unwrap_or(&settings.databases),
            *no_trim,
            *dry_run,
        ),
    }
}

fn catalog(ctx: &Context, settings: &SourcesSettings) -> Result<SqlCatalog> {
    let session = ctx.service::<Arc<dyn SqlSession>>()?;
    Ok(SqlCatalog::new(session, &settings.column_order))
}

fn undefined(done: &vdp_dbt::Completed) -> Vec<AddRequest> {
    let text = format!(
        "{}\n{}",
        done.stdout_text(),
        String::from_utf8_lossy(&done.stderr)
    );
    missing_sources(&text)
        .into_iter()
        .map(|m| AddRequest {
            source: m.source,
            table: m.table,
        })
        .collect()
}

fn add(
    ctx: &Context,
    settings: &SourcesSettings,
    tables: &[String],
    missing: bool,
    dry_run: bool,
) -> Result<Outcome> {
    let project = ctx.service::<Arc<DbtProject>>()?;
    let mut requests = tables
        .iter()
        .map(|t| AddRequest::parse(t))
        .collect::<Result<Vec<_>, _>>()?;
    if missing {
        let done = parse(ctx, &project, true)?;
        let found = undefined(&done);
        if found.is_empty() {
            if done.success() {
                println!("dbt reports no undefined sources");
            } else {
                print_dbt_errors(&done);
                bail!("dbt parse failed, but not because of undefined sources");
            }
        }
        for request in found {
            if !requests.contains(&request) {
                println!("dbt reports undefined source {request}");
                requests.push(request);
            }
        }
    }
    if requests.is_empty() {
        if missing {
            return Ok(Outcome::Success);
        }
        bail!("nothing to add: give database.table arguments or --missing");
    }

    let catalog = catalog(ctx, settings)?;
    let report = vdp_sources::add(AddOptions {
        project_root: &project.root,
        model_paths: &project.model_paths,
        settings,
        catalog: &catalog,
        requests: &requests,
        dry_run,
    })?;
    let verb = if dry_run { "would add" } else { "added" };
    for record in &report {
        let request = &record.key;
        match &record.outcome {
            BatchOutcome::Succeeded(AddSuccess::Added { file, columns }) => println!(
                "{verb} {request} ({columns} columns) to {}",
                relative(&project.root, file)
            ),
            BatchOutcome::Succeeded(AddSuccess::Refreshed { file }) => {
                println!(
                    "refreshed the columns of {request} in {}",
                    relative(&project.root, file)
                );
            }
            BatchOutcome::Succeeded(AddSuccess::AlreadyDeclared { file }) => {
                println!(
                    "{request} is already declared in {}",
                    relative(&project.root, file)
                );
            }
            BatchOutcome::Failed(AddFailure::NotInDatabase {
                database,
                suggestions,
            }) => {
                let hint = if suggestions.is_empty() {
                    String::new()
                } else {
                    format!("; did you mean {}?", suggestions.join(", "))
                };
                println!(
                    "error: database `{database}` has no table `{}`{hint}",
                    request.table
                );
            }
            BatchOutcome::Failed(AddFailure::Unsupported { file, reason }) => {
                println!(
                    "error: {request}: {} ({reason})",
                    relative(&project.root, file)
                );
            }
            // `add` never skips a request.
            BatchOutcome::Skipped { .. } => {}
        }
    }
    let mut ok = !report.has_failures();
    if missing
        && !dry_run
        && report
            .succeeded()
            .any(|(_, success)| matches!(success, AddSuccess::Added { .. }))
    {
        let done = parse(ctx, &project, true)?;
        let still = undefined(&done);
        if still.is_empty() {
            println!("dbt parse: no undefined sources left");
        } else {
            ok = false;
            let names: Vec<String> = still.iter().map(ToString::to_string).collect();
            println!("dbt still reports undefined sources: {}", names.join(", "));
        }
    }
    Ok(Outcome::from_success(ok))
}

fn format_refreshed(root: &std::path::Path, entry: &Refreshed, dry_run: bool) -> String {
    match entry {
        Refreshed::Written {
            database,
            file,
            tables,
            status,
        } if dry_run => format!(
            "{database}: {tables} tables into {} ({})",
            relative(root, file),
            status.as_str()
        ),
        Refreshed::Written {
            database,
            file,
            tables,
            ..
        } => format!("{database}: {tables} tables into {}", relative(root, file)),
        Refreshed::SkippedHandWritten { database, file } => format!(
            "{database}: skipped, declared by hand in {}",
            relative(root, file)
        ),
        Refreshed::Empty { database } => format!("{database}: no tables (does it exist?)"),
    }
}

fn format_trimmed(root: &std::path::Path, entry: &Trimmed) -> String {
    let rel = |file: &std::path::Path| relative(root, file);
    match entry {
        Trimmed::Kept { file, tables } => {
            format!("kept {tables} referenced tables in {}", rel(file))
        }
        Trimmed::Deleted { file } => format!("deleted {} (nothing referenced)", rel(file)),
    }
}

fn rewrite(
    ctx: &Context,
    settings: &SourcesSettings,
    databases: &[String],
    no_trim: bool,
    dry_run: bool,
) -> Result<Outcome> {
    let project = ctx.service::<Arc<DbtProject>>()?;
    let catalog = catalog(ctx, settings)?;
    let report = vdp_sources::rewrite::refresh(RefreshOptions {
        project_root: &project.root,
        model_paths: &project.model_paths,
        settings,
        catalog: &catalog,
        databases,
        dry_run,
    })?;
    for entry in &report {
        println!("{}", format_refreshed(&project.root, entry, dry_run));
    }
    if dry_run {
        if !no_trim {
            println!("dry-run: skipping dbt parse; trim happens on the real run");
        }
        return Ok(Outcome::Success);
    }
    if no_trim {
        return Ok(Outcome::Success);
    }
    let done = parse(ctx, &project, false)?;
    if !done.success() {
        print_dbt_errors(&done);
        bail!("dbt parse failed; the regenerated files are kept untrimmed");
    }
    let manifest = Manifest::load(&project.manifest_path()).context("reading the manifest")?;
    for entry in vdp_sources::rewrite::trim(TrimOptions {
        project_root: &project.root,
        settings,
        manifest: &manifest,
        databases,
    })? {
        println!("{}", format_trimmed(&project.root, &entry));
    }
    Ok(Outcome::Success)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use vdp_core::Config;
    use vdp_db::{DbError, Row, SqlSession, SqlValue};
    use vdp_dbt::{Completed, DbtEngine, DbtError, DbtProject, Invocation};

    use super::*;
    use crate::bootstrap::DbtSelection;

    #[derive(Debug, Default)]
    struct FakeCatalogSession {
        queries: Mutex<Vec<String>>,
    }

    impl SqlSession for FakeCatalogSession {
        fn execute(&self, _sql: &str) -> std::result::Result<(), DbError> {
            Ok(())
        }

        fn execute_with(
            &self,
            _sql: &str,
            _params: &[SqlValue],
        ) -> std::result::Result<(), DbError> {
            Ok(())
        }

        fn query(&self, _sql: &str, params: &[SqlValue]) -> std::result::Result<Vec<Row>, DbError> {
            let db = params.first().and_then(SqlValue::as_text).unwrap_or("");
            self.queries.lock().unwrap().push(db.to_owned());
            let rows = match db {
                "fresh_db" => vec![vec![SqlValue::from("tbl_fresh"), SqlValue::from("id")]],
                "changed_db" => vec![
                    vec![SqlValue::from("tbl_old"), SqlValue::from("id")],
                    vec![SqlValue::from("tbl_new"), SqlValue::from("id")],
                ],
                "dev_landing" => vec![
                    vec![SqlValue::from("tbl_a"), SqlValue::from("id")],
                    vec![SqlValue::from("tbl_b"), SqlValue::from("id")],
                ],
                _ => Vec::new(),
            };
            Ok(rows)
        }
    }

    #[derive(Debug)]
    struct FakeDbtEngine {
        binary: PathBuf,
        invocations: Mutex<Vec<Vec<std::ffi::OsString>>>,
    }

    impl FakeDbtEngine {
        fn new(binary: PathBuf) -> Self {
            Self {
                binary,
                invocations: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Vec<std::ffi::OsString>> {
            self.invocations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    impl DbtEngine for FakeDbtEngine {
        fn binary(&self) -> &Path {
            &self.binary
        }

        fn invoke(&self, invocation: &Invocation) -> std::result::Result<Completed, DbtError> {
            self.invocations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(invocation.args.clone());
            Ok(Completed {
                code: Some(0),
                signal: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    const LANDING_SEED: &str = "version: 2\nsources:\n  - name: dev_landing\n    database: dev_landing\n    schema: dev_landing\n    tables:\n      - name: tbl_a\n        columns:\n          - name: id\n      - name: tbl_b\n        columns:\n          - name: id\n";
    const CHANGED_SEED: &str = "version: 2\nsources:\n  - name: changed_db\n    database: changed_db\n    schema: changed_db\n    tables:\n      - name: tbl_old\n        columns:\n          - name: id\n";

    fn seed_rewrite_ctx(root: &Path) -> (Context, Arc<FakeDbtEngine>, [PathBuf; 3]) {
        std::fs::write(root.join("dbt_project.yml"), "name: demo\n").unwrap();
        std::fs::create_dir_all(root.join("models/sources")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();

        let fresh_file = root.join("models/sources/sources_fresh_db.yml");
        let changed_file = root.join("models/sources/sources_changed_db.yml");
        let landing_file = root.join("models/sources/sources_dev_landing.yml");
        std::fs::write(&changed_file, CHANGED_SEED).unwrap();
        std::fs::write(&landing_file, LANDING_SEED).unwrap();

        std::fs::write(
            root.join("target/manifest.json"),
            r#"{
              "nodes": {
                "model.p.m": {
                  "unique_id": "model.p.m",
                  "resource_type": "model",
                  "name": "m",
                  "database": "db1",
                  "depends_on": {"nodes": ["source.p.dev_landing.tbl_a"]}
                }
              },
              "sources": {
                "source.p.dev_landing.tbl_a": {
                  "unique_id": "source.p.dev_landing.tbl_a",
                  "source_name": "dev_landing",
                  "name": "tbl_a",
                  "database": "dev_landing"
                }
              }
            }"#,
        )
        .unwrap();

        let project = Arc::new(DbtProject::load(root).unwrap());
        let fake_dbt = Arc::new(FakeDbtEngine::new(root.join("dbt")));
        let engine: Arc<dyn DbtEngine> = fake_dbt.clone();
        let session: Arc<dyn SqlSession> = Arc::new(FakeCatalogSession::default());
        let ctx = Context::builder(Config::default())
            .provide_value(project)
            .provide_value(engine)
            .provide_value(session)
            .provide_value(DbtSelection::default())
            .build();
        (ctx, fake_dbt, [fresh_file, changed_file, landing_file])
    }

    #[test]
    fn sources_rewrite_dry_run_diffs_in_memory_and_skips_dbt_parse() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (ctx, fake_dbt, [fresh_file, changed_file, landing_file]) = seed_rewrite_ctx(root);

        let dbs = vec![
            "fresh_db".to_owned(),
            "changed_db".to_owned(),
            "dev_landing".to_owned(),
        ];
        let dry = run(
            &ctx,
            &SourcesCommand::Rewrite {
                databases: Some(dbs.clone()),
                no_trim: false,
                dry_run: true,
            },
        )
        .unwrap();
        assert_eq!(dry, Outcome::Success);
        assert!(
            fake_dbt.calls().is_empty(),
            "sources rewrite --dry-run invoked dbt: {:?}",
            fake_dbt.calls()
        );
        assert!(!fresh_file.exists(), "created file was written in dry-run");
        assert_eq!(
            std::fs::read(&changed_file).unwrap(),
            CHANGED_SEED.as_bytes()
        );
        assert_eq!(
            std::fs::read(&landing_file).unwrap(),
            LANDING_SEED.as_bytes()
        );

        let project = ctx.service::<Arc<DbtProject>>().unwrap();
        let settings: SourcesSettings = ctx.config().section(SourcesSettings::SECTION).unwrap();
        let cat = catalog(&ctx, &settings).unwrap();
        let report = vdp_sources::rewrite::refresh(RefreshOptions {
            project_root: &project.root,
            model_paths: &project.model_paths,
            settings: &settings,
            catalog: &cat,
            databases: &dbs,
            dry_run: true,
        })
        .unwrap();
        let lines: Vec<String> = report
            .iter()
            .map(|e| format_refreshed(root, e, true))
            .collect();
        assert_eq!(
            lines,
            vec![
                "fresh_db: 1 tables into models/sources/sources_fresh_db.yml (created)",
                "changed_db: 2 tables into models/sources/sources_changed_db.yml (changed)",
                "dev_landing: 2 tables into models/sources/sources_dev_landing.yml (unchanged)",
            ]
        );
        assert_eq!(
            format_trimmed(
                root,
                &Trimmed::Kept {
                    file: landing_file.clone(),
                    tables: 1,
                },
            ),
            "kept 1 referenced tables in models/sources/sources_dev_landing.yml"
        );

        let live = run(
            &ctx,
            &SourcesCommand::Rewrite {
                databases: Some(dbs),
                no_trim: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(live, Outcome::Success);
        assert_eq!(fake_dbt.calls().len(), 1);
        let updated_landing = std::fs::read_to_string(&landing_file).unwrap();
        assert!(updated_landing.contains("- name: tbl_a"));
        assert!(!updated_landing.contains("- name: tbl_b"));
        assert!(!changed_file.exists());
    }
}
