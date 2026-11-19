//! `vdp-vaultspeed vaultspeed deploy|plan|ledger|rules|dags|history`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use similar::TextDiff;
use vdp_core::{Context, Environment, Outcome};
use vdp_vaultspeed::ledger::LegacyImporter;
use vdp_vaultspeed::report::ReportSink;
use vdp_vaultspeed::sources::load_path;
use vdp_vaultspeed::wiring::{self, EngineInputs, Project, ReportSinkOptions, RunOptions};
use vdp_vaultspeed_dags::{DagDocument, DagNamer, VaultSpeedDagNamer};
use vdp_vaultspeed_domain::{ChangeRecorder, NoHistory, ObjectRef, RunId, RunMode, RunReport};
use vdp_vaultspeed_history::DuckDbHistory;

use vdp_cli::args::GlobalArgs;

use super::history;
use crate::args::{
    RuleArtifact, VaultspeedArgs, VaultspeedCommand, VaultspeedDagsCommand,
    VaultspeedLedgerCommand, VaultspeedRulesCommand, VaultspeedRunArgs,
};
use crate::bootstrap as infra;

/// `--env` wins; otherwise `$TARGET_BRANCH`, which the framework's CI sets to the environment branch.
///
/// # Errors
///
/// An error when `$TARGET_BRANCH` is set but is not a valid environment name.
pub fn environment_override(global: &GlobalArgs) -> Result<Option<Environment>> {
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

/// Runs a `vaultspeed` command in a context made for the environment [`environment_override`] gave.
///
/// # Errors
///
/// An error when the framework checkout cannot be set up, the infrastructure cannot be reached, or the command fails.
/// A run that applies with failures is not an error: it is the [`Outcome`].
pub fn run(ctx: &Context, global: &GlobalArgs, args: &VaultspeedArgs) -> Result<Outcome> {
    if let VaultspeedCommand::History(history_args) = &args.command {
        return history::run(ctx, args.framework_dir.as_deref(), history_args);
    }
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
        VaultspeedCommand::History(_) => unreachable!("handled above"),
    }
}

/// What [`deploy`] asks [`infra::infrastructure`] for: `--offline` from the run args, Vault from the global
/// `--no-vault`. Pure, so the two bools cannot be swapped unnoticed. Both the `deploy` and the `plan` command run
/// through [`deploy`] (`plan` is a preview); the ledger commands build their own [`infra::InfrastructureOptions`].
fn deploy_infrastructure_options(
    environment: Environment,
    args: &VaultspeedRunArgs,
    mode: RunMode,
    use_vault: bool,
) -> infra::InfrastructureOptions {
    infra::InfrastructureOptions {
        environment,
        offline: args.offline,
        use_vault,
        mode,
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
    let resolved = infra::infrastructure(
        ctx,
        project,
        deploy_infrastructure_options(environment, args, mode, use_vault),
    )?;
    let options = RunOptions {
        environment,
        mode,
        only: args.files.clone(),
        git_sha: std::env::var("CI_COMMIT_SHA").ok(),
    };
    let history = open_history(project, mode, args.offline);
    let report = wiring::engine_with_history(
        project,
        EngineInputs {
            options: &options,
            infra: &resolved.infra,
            history,
        },
    )?
    .run()?;

    let sinks = wiring::report_sinks(
        project,
        ReportSinkOptions {
            explain: args.explain,
            report_out: args.report_out.as_ref().map(|p| ctx.cwd().join(p)),
            junit: args.junit.as_ref().map(|p| ctx.cwd().join(p)),
        },
    );
    write_reports(&report, &sinks);
    Ok(if report.has_failures() {
        Outcome::Failures
    } else {
        Outcome::Success
    })
}

/// The object history of an applying run: the DuckDB file when `[vaultspeed.history].enabled`. A preview or an
/// offline run records nothing, and a history that cannot be opened is a warning: it never blocks a deployment.
fn open_history(project: &Project, mode: RunMode, offline: bool) -> Arc<dyn ChangeRecorder> {
    if mode.is_preview() || offline || !project.config.history.enabled {
        return Arc::new(NoHistory);
    }
    let path = project.history_path();
    match DuckDbHistory::open(&path) {
        Ok(history) => Arc::new(history),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %vdp_core::display_chain(&error),
                "object history unavailable; continuing without it"
            );
            Arc::new(NoHistory)
        }
    }
}

fn write_reports(report: &RunReport, sinks: &[Box<dyn ReportSink>]) {
    for sink in sinks {
        match sink.write(report) {
            Ok(paths) => paths
                .iter()
                .for_each(|p| tracing::info!(path = %p.display(), "report written")),
            Err(error) => {
                tracing::error!(error = %vdp_core::display_chain(&error), "could not write report");
            }
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
        infra::InfrastructureOptions {
            environment,
            offline,
            use_vault,
            mode: RunMode::Apply,
        },
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
        infra::InfrastructureOptions {
            environment,
            offline,
            use_vault,
            mode: RunMode::Preview,
        },
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
    let files = load_path(path)?;
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
                tracing::error!("# {name}: {}", vdp_core::display_chain(&error));
            }
        }
    }
    Ok(if failed {
        Outcome::Failures
    } else {
        Outcome::Success
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(offline: bool) -> VaultspeedRunArgs {
        VaultspeedRunArgs {
            offline,
            files: None,
            preview: false,
            explain: false,
            report_out: None,
            junit: None,
        }
    }

    #[test]
    fn deploy_options_map_each_field_from_its_own_source() {
        let offline_no_vault = deploy_infrastructure_options(
            Environment::PRD,
            &run_args(true),
            RunMode::Preview,
            false,
        );
        assert_eq!(offline_no_vault.environment, Environment::PRD);
        assert!(offline_no_vault.offline, "offline comes from --offline");
        assert!(
            !offline_no_vault.use_vault,
            "use_vault comes from the global flag"
        );
        assert_eq!(offline_no_vault.mode, RunMode::Preview);

        let online_vault =
            deploy_infrastructure_options(Environment::LDV, &run_args(false), RunMode::Apply, true);
        assert_eq!(online_vault.environment, Environment::LDV);
        assert!(!online_vault.offline);
        assert!(online_vault.use_vault);
        assert_eq!(online_vault.mode, RunMode::Apply);
    }
}
