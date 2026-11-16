//! `vdp vaultspeed deploy|plan|ledger|rules|dags`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use similar::TextDiff;
use vdp_core::{Context, Environment, Outcome};
use vdp_vaultspeed::ledger::LegacyImporter;
use vdp_vaultspeed::report::{ConsoleSink, ErrorCsvSink, JsonReportSink, JunitSink, ReportSink};
use vdp_vaultspeed::sources::{ArtifactReader, DirectoryReader, SourceFile, ZipArchiveReader};
use vdp_vaultspeed::wiring::{self, Project, RunOptions};
use vdp_vaultspeed_dags::{DagDocument, DagNamer, VaultSpeedDagNamer};
use vdp_vaultspeed_domain::{ObjectRef, RelPath, RunId, RunMode, RunReport};

use crate::args::{
    GlobalArgs, RuleArtifact, VaultspeedArgs, VaultspeedCommand, VaultspeedDagsCommand,
    VaultspeedLedgerCommand, VaultspeedRulesCommand, VaultspeedRunArgs,
};
use crate::bootstrap::vaultspeed as infra;

/// `--env` wins; otherwise `$TARGET_BRANCH`, which the framework's CI sets to the environment branch.
pub(super) fn environment_override(global: &GlobalArgs) -> Result<Option<Environment>> {
    if global.environment.is_some() {
        return Ok(global.environment);
    }
    match std::env::var("TARGET_BRANCH") {
        Ok(value) if !value.trim().is_empty() => Ok(Some(
            value
                .parse()
                .with_context(|| format!("$TARGET_BRANCH ({value})"))?,
        )),
        _ => Ok(None),
    }
}

pub(super) fn run(ctx: &Context, global: &GlobalArgs, args: &VaultspeedArgs) -> Result<Outcome> {
    let project = infra::project(ctx, args.framework_dir.as_deref())?;
    let use_vault = !global.no_vault;
    let environment = ctx.environment();
    match &args.command {
        VaultspeedCommand::Deploy(run) => deploy(ctx, &project, run, run.preview, use_vault),
        VaultspeedCommand::Plan(run) => deploy(ctx, &project, run, true, use_vault),
        VaultspeedCommand::Ledger(VaultspeedLedgerCommand::Import { offline, force }) => {
            ledger_import(ctx, &project, environment, *offline, use_vault, *force)
        }
        VaultspeedCommand::Ledger(VaultspeedLedgerCommand::Stats { offline }) => {
            ledger_stats(ctx, &project, environment, *offline, use_vault)
        }
        VaultspeedCommand::Rules(VaultspeedRulesCommand::Test { file, artifact }) => {
            rules_test(&project, &ctx.cwd().join(file), *artifact)
        }
        VaultspeedCommand::Dags(VaultspeedDagsCommand::Render { path, host }) => {
            dags_render(&project, &ctx.cwd().join(path), environment, host.clone())
        }
    }
}

fn deploy(
    ctx: &Context,
    project: &Project,
    args: &VaultspeedRunArgs,
    preview: bool,
    use_vault: bool,
) -> Result<Outcome> {
    let environment = ctx.environment();
    if args.offline && !preview {
        bail!("--offline cannot apply changes; use `vdp vaultspeed plan` or `--preview`");
    }
    let mode = if preview {
        RunMode::Preview
    } else {
        RunMode::Apply
    };
    let resolved = infra::infrastructure(ctx, project, environment, args.offline, use_vault, mode)?;
    let options = RunOptions {
        environment,
        mode,
        only: args.files.clone(),
        git_sha: std::env::var("CI_COMMIT_SHA").ok(),
    };
    let report = wiring::engine(project, &options, &resolved.infra)?.run()?;

    let mut sinks: Vec<Box<dyn ReportSink>> = vec![
        Box::new(ConsoleSink::stdout(args.explain)),
        Box::new(JsonReportSink::in_dir(project.root.join("logs"))),
        Box::new(ErrorCsvSink::new(project.layout.logs_dir())),
    ];
    if let Some(path) = &args.report_out {
        sinks.push(Box::new(JsonReportSink::to_file(ctx.cwd().join(path))));
    }
    if let Some(path) = &args.junit {
        sinks.push(Box::new(JunitSink::new(ctx.cwd().join(path))));
    }
    write_reports(&report, &sinks);
    Ok(if report.has_failures() {
        Outcome::Failures
    } else {
        Outcome::Success
    })
}

fn write_reports(report: &RunReport, sinks: &[Box<dyn ReportSink>]) {
    for sink in sinks {
        match sink.write(report) {
            Ok(paths) => paths
                .iter()
                .for_each(|p| tracing::info!(path = %p.display(), "report written")),
            Err(error) => tracing::error!(%error, "could not write report"),
        }
    }
}

fn ledger_import(
    ctx: &Context,
    project: &Project,
    environment: Environment,
    offline: bool,
    use_vault: bool,
    force: bool,
) -> Result<Outcome> {
    let resolved = infra::infrastructure(
        ctx,
        project,
        environment,
        offline,
        use_vault,
        RunMode::Apply,
    )?;
    let ledger = &resolved.infra.ledger;
    let existing = ledger.last_sequence()?;
    if existing > 0 && !force {
        bail!(
            "the {environment} ledger already has {existing} entries; pass --force to import anyway"
        );
    }
    let stats = LegacyImporter::default().import(
        &project.layout.deployed_dir(),
        &RunId::from_string(format!("legacy-import-{}", RunId::generate())),
        ledger.as_ref(),
    )?;
    for (artifact, files) in &stats.files {
        println!(
            "{artifact}: {files} files, {} statements imported",
            stats.statements.get(artifact).unwrap_or(&0)
        );
    }
    Ok(Outcome::Success)
}

fn ledger_stats(
    ctx: &Context,
    project: &Project,
    environment: Environment,
    offline: bool,
    use_vault: bool,
) -> Result<Outcome> {
    let resolved = infra::infrastructure(
        ctx,
        project,
        environment,
        offline,
        use_vault,
        RunMode::Preview,
    )?;
    let ledger = &resolved.infra.ledger;
    let entries = ledger.entries()?;
    let mut by_outcome: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_source: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &entries {
        *by_outcome.entry(entry.outcome.as_str()).or_default() += 1;
        *by_source.entry(entry.source.as_str()).or_default() += 1;
    }
    println!(
        "{environment} ledger: {} entries (last sequence {})",
        entries.len(),
        ledger.last_sequence()?
    );
    for (outcome, count) in by_outcome {
        println!("  {outcome:<10} {count}");
    }
    println!("by source:");
    for (source, count) in by_source {
        println!("  {source:<24} {count}");
    }
    Ok(Outcome::Success)
}

fn rules_test(project: &Project, file: &Path, artifact: RuleArtifact) -> Result<Outcome> {
    let pipelines = wiring::pipelines(project)?;
    let pipeline = match artifact {
        RuleArtifact::Ddl => &pipelines.ddl,
        RuleArtifact::Etl => &pipelines.etl,
    };
    let text = fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    for (index, statement) in pipeline.statements(&text)?.iter().enumerate() {
        println!(
            "#{index} {} {} rules: [{}] raw {}",
            statement.kind.label(),
            statement
                .target
                .as_ref()
                .map(ObjectRef::qualified_name)
                .unwrap_or_default(),
            statement.applied_rules.join(", "),
            statement.raw_fp.short(),
        );
        let diff = TextDiff::from_lines(statement.raw.as_str(), statement.effective.as_str());
        print!("{}", diff.unified_diff().header("raw", "effective"));
    }
    Ok(Outcome::Success)
}

fn dags_render(
    project: &Project,
    path: &Path,
    environment: Environment,
    host: Option<String>,
) -> Result<Outcome> {
    let files: Vec<SourceFile> = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        ZipArchiveReader::new(path.to_owned()).read()?
    } else if path.is_dir() {
        DirectoryReader::new(path.to_owned()).read()?
    } else {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        vec![SourceFile::new(
            RelPath::new(name),
            fs::read_to_string(path)?,
        )]
    };
    let host = host.unwrap_or_else(|| format!("singlestore-rel1{}.invalid", environment.lower()));
    let pipeline = wiring::dag_pipeline(project)?;
    let context = wiring::dag_context(project, environment, &host)?;
    let namer = Arc::new(VaultSpeedDagNamer);
    let mut failed = false;
    for file in files.iter().filter(|f| f.path.has_extension("py")) {
        let name = file.path.file_name();
        match DagDocument::parse(name, &file.content).and_then(|doc| pipeline.render(doc, &context))
        {
            Ok(rendered) => {
                println!(
                    "# {name} -> {} [{}]",
                    namer.target_name(name),
                    rendered.applied.join(", ")
                );
                let diff = TextDiff::from_lines(file.content.as_str(), rendered.document.text());
                print!(
                    "{}",
                    diff.unified_diff().header(name, &namer.target_name(name))
                );
            }
            Err(error) => {
                failed = true;
                eprintln!("# {name}: {error}");
            }
        }
    }
    Ok(if failed {
        Outcome::Failures
    } else {
        Outcome::Success
    })
}
