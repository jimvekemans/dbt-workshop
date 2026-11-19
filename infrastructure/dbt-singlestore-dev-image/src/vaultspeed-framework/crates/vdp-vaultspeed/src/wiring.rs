//! Turns the `[vaultspeed]` settings into components: the only place that knows which implementation backs which
//! abstraction. The CLI (`vdp vaultspeed`) and the release hub (plan previews) both build their engines here, so
//! they decide the same way. Acquiring infrastructure (secrets, the database session, the Airflow transport) is
//! the caller's job; [`Infrastructure`] is what it hands in.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use vdp_db::{ConnectionSettings, SqlSession, TlsMode};
use vdp_secrets::{ExposeSecret, SecretProvider, first_of};
use vdp_ssh::{RemoteTransport, SecretNames};
use vdp_vaultspeed_dags::{
    CompositePublisher, DagAlterationPipeline, DagContext, DagPublisher, DagTransformer,
    DefaultArgsEntry, EnsureImport, LoadSuccessOutlet, MappingTaskOutlets, PyLiteral,
    RepoDirPublisher, SftpPublisher, SourceResolver, SyntaxValidator, VaultSpeedDagNamer,
};

use crate::config::{
    ConfigError, FailurePolicySetting, LedgerBackend, SnapshotSetting, StandaloneDropsSetting,
    VaultspeedSettings, load_fmc_sources,
};
use crate::engine::{DagStage, DagStageParts, Engine, EngineParts, FailurePolicies};
use crate::executor::{
    DryRunExecutor, ErrorClassifier, FailurePolicy, InformationSchemaProbe, MySqlErrorClassifier,
    NoClassification, PlanRunner, PlanRunnerParts, RunContext, SessionExecutor, StatementExecutor,
};
use crate::ledger::{
    CompositeLedger, JsonlLedger, Ledger, LedgerSink, LedgerTable, OverlayLedger, SqlLedger,
};
use crate::planner::{
    AlwaysRunPolicy, CatalogProbe, DdlPhases, DowngradeGuard, EtlPhases, ExclusionPolicy,
    ExecutionGuard, IdempotencyPolicy, OnceOnlyPolicy, Planner, PmlPhases, RebuildGuard,
    ReplaceablePolicy, SequentialPhasing, StandaloneDrops,
};
use crate::report::{ConsoleSink, ErrorCsvSink, JsonReportSink, JunitSink, ReportSink};
use crate::sources::{
    ArtifactLoader, DeployedSnapshot, DropCreatePairing, GenerationHandler, PerFileGrouping,
    PmlDiscovery, PmlHandler, PmlLifecycle, ReleaseZipDiscovery, SnapshotMode, SourceDiscovery,
    SourceHandler, SourceLifecycle, StagingLayout, ZipArchiveLifecycle,
};
use vdp_vaultspeed_domain::{
    ArtifactKind, ChangeRecorder, Environment, GenerationKind, Layer, NoHistory, RunId, RunMode,
};
use vdp_vaultspeed_sql::{
    ArtifactPipeline, NoRewrite, RoutineAwareSplitter, RuleSet, SqlPreprocessor, StripComments,
};

/// Errors while wiring a run.
#[derive(Debug, thiserror::Error)]
pub enum WiringError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("loading rewrite rules from {path}")]
    Rules {
        path: PathBuf,
        #[source]
        source: vdp_vaultspeed_sql::SqlError,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("missing secret: {0}")]
    MissingSecret(String),
    #[error(transparent)]
    Ledger(#[from] crate::ledger::LedgerError),
}

type Result<T, E = WiringError> = std::result::Result<T, E>;

/// The framework checkout on disk: settings plus directory layout.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: VaultspeedSettings,
    pub layout: StagingLayout,
    pub fmc_sources: Vec<String>,
}

impl Project {
    /// The checkout at `root` with these settings (validated) and its `config/vaultspeed.json`.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] when the settings fail [`VaultspeedSettings::validate`]; [`ConfigError::Io`] or
    /// [`ConfigError::FmcSources`] when the source list cannot be read.
    pub fn with_config(root: &Path, config: VaultspeedSettings) -> Result<Self, ConfigError> {
        config.validate()?;
        let fmc_sources = load_fmc_sources(&root.join(&config.paths.fmc_sources_file))?;
        Ok(Self {
            layout: StagingLayout::new(root.join(&config.paths.vaultspeed_dir)),
            root: root.to_owned(),
            config,
            fmc_sources,
        })
    }

    /// The file `path`: as given when absolute or present under the checkout, else in the configured assets directory.
    pub fn asset(&self, path: &Path) -> PathBuf {
        if path.is_absolute() || self.root.join(path).exists() {
            self.root.join(path)
        } else {
            self.root.join(&self.config.paths.assets_dir).join(path)
        }
    }

    /// The local object history database (`[vaultspeed.history].path` under the checkout).
    pub fn history_path(&self) -> PathBuf {
        self.root.join(&self.config.history.path)
    }

    /// The JSONL mirror of the ledger of `environment`.
    pub fn jsonl_ledger_path(&self, environment: Environment) -> PathBuf {
        self.layout
            .ledger_dir()
            .join(format!("{environment}.jsonl"))
    }
}

/// SQL pipelines per artifact kind.
#[derive(Debug, Clone)]
pub struct Pipelines {
    pub ddl: ArtifactPipeline,
    pub etl: ArtifactPipeline,
    pub pml: ArtifactPipeline,
}

/// DDL: split, strip comments (unless they carry SHARD KEY hints), DDL rules.
/// ETL: split, ETL rules. PML: split only; it is written for the target platform.
///
/// # Errors
///
/// [`WiringError::Rules`] when a rewrite-rule file cannot be read or parsed.
pub fn pipelines(project: &Project) -> Result<Pipelines> {
    pipelines_in(project, "")
}

/// [`pipelines`] for one environment: unqualified object names in the SQL resolve to the environment's
/// database (`[vaultspeed.environments.<env>].database`), so they are the same objects as their qualified
/// spellings in the ledger.
///
/// # Errors
///
/// As [`pipelines`].
pub fn pipelines_for(project: &Project, environment: Environment) -> Result<Pipelines> {
    pipelines_in(project, &project.config.environment(environment).database)
}

/// [`pipelines`] with `database` as the database in effect when a script has no `USE` (empty: unknown).
///
/// # Errors
///
/// As [`pipelines`].
pub fn pipelines_in(project: &Project, database: &str) -> Result<Pipelines> {
    let rules = |artifact| {
        let dir = project.layout.rules_dir(artifact);
        RuleSet::from_dir(&dir).map_err(|source| WiringError::Rules { path: dir, source })
    };
    let splitter = Arc::new(RoutineAwareSplitter);
    let strip: Arc<dyn SqlPreprocessor> = Arc::new(StripComments::unless_shard_key_hint());
    Ok(Pipelines {
        ddl: ArtifactPipeline::new(
            splitter.clone(),
            vec![strip],
            Arc::new(rules(ArtifactKind::Ddl)?),
        )
        .with_default_database(database),
        etl: ArtifactPipeline::new(
            splitter.clone(),
            vec![],
            Arc::new(rules(ArtifactKind::Etl)?),
        )
        .with_default_database(database),
        pml: ArtifactPipeline::new(splitter, vec![], Arc::new(NoRewrite))
            .with_default_database(database),
    })
}

/// One handler per source kind, composed from loaders.
pub fn handlers(config: &VaultspeedSettings, pipelines: &Pipelines) -> Vec<Arc<dyn SourceHandler>> {
    let ddl = Arc::new(ArtifactLoader::new(
        ArtifactKind::Ddl,
        pipelines.ddl.clone(),
        Arc::new(DropCreatePairing),
    ));
    let etl = Arc::new(ArtifactLoader::new(
        ArtifactKind::Etl,
        pipelines.etl.clone(),
        Arc::new(PerFileGrouping),
    ));
    let pml = Arc::new(ArtifactLoader::new(
        ArtifactKind::Pml,
        pipelines.pml.clone(),
        Arc::new(PerFileGrouping),
    ));
    let mut comp = GenerationHandler::new(GenerationKind::Comp)
        .with_sql("DDL", ddl.clone())
        .with_sql("ETL", etl.clone());
    if config.dags.publish_comp_dags {
        comp = comp.with_dags("");
    }
    vec![
        Arc::new(PmlHandler::new(pml)),
        Arc::new(GenerationHandler::new(GenerationKind::Ddl).with_sql("", ddl)),
        Arc::new(GenerationHandler::new(GenerationKind::Etl).with_sql("", etl)),
        Arc::new(comp),
        Arc::new(GenerationHandler::new(GenerationKind::Fmc).with_dags("")),
    ]
}

/// Decision chain: exclusions, staged PML, replaceable objects, run-once fallback.
pub fn planner(config: &VaultspeedSettings, probe: Option<Arc<dyn CatalogProbe>>) -> Planner {
    let planning = &config.planning;
    let probe = probe.filter(|_| planning.catalog_probe);
    let standalone_drops = match planning.standalone_drops {
        StandaloneDropsSetting::Skip => StandaloneDrops::Skip,
        StandaloneDropsSetting::Execute => StandaloneDrops::Execute,
    };
    let policies: Vec<Arc<dyn IdempotencyPolicy>> = vec![
        Arc::new(ExclusionPolicy::new(
            planning.excluded_job_substrings.clone(),
            planning.excluded_path_substrings.clone(),
            standalone_drops,
        )),
        Arc::new(AlwaysRunPolicy::new(vec![ArtifactKind::Pml])),
        Arc::new(ReplaceablePolicy::new(probe.clone())),
        Arc::new(OnceOnlyPolicy::new(probe)),
    ];
    let phasing = SequentialPhasing::new(vec![
        Box::new(PmlPhases),
        Box::new(DdlPhases::new(planning.rdv_order_numbers.clone())),
        Box::new(EtlPhases::new(
            planning
                .etl_layer_priority
                .iter()
                .map(|l| Layer::from_dir_name(l))
                .collect(),
        )),
    ]);
    let mut guards: Vec<Arc<dyn ExecutionGuard>> = vec![Arc::new(RebuildGuard::new(
        planning.protected_rebuild_order_numbers.clone(),
    ))];
    if !planning.allow_downgrades {
        guards.push(Arc::new(DowngradeGuard));
    }
    Planner::new(policies, guards, Arc::new(phasing))
}

/// The configured failure policy for each artifact kind.
pub fn failure_policies(config: &VaultspeedSettings) -> FailurePolicies {
    let map = |setting| match setting {
        FailurePolicySetting::Continue => FailurePolicy::Continue,
        FailurePolicySetting::HaltSource => FailurePolicy::HaltSource,
        FailurePolicySetting::HaltRun => FailurePolicy::HaltRun,
    };
    FailurePolicies {
        pml: map(config.failure.pml),
        ddl: map(config.failure.ddl),
        etl: map(config.failure.etl),
    }
}

/// The `deployed/` mirror as configured; `None` when it is disabled for every artifact kind.
pub fn snapshot(project: &Project) -> Option<DeployedSnapshot> {
    let settings = &project.config.snapshot;
    let modes: BTreeMap<_, _> = [
        (ArtifactKind::Ddl, settings.ddl),
        (ArtifactKind::Etl, settings.etl),
        (ArtifactKind::Pml, settings.pml),
    ]
    .into_iter()
    .filter_map(|(artifact, setting)| match setting {
        SnapshotSetting::Append => Some((artifact, SnapshotMode::Append)),
        SnapshotSetting::Overwrite => Some((artifact, SnapshotMode::Overwrite)),
        SnapshotSetting::Disabled => None,
    })
    .collect();
    (!modes.is_empty()).then(|| DeployedSnapshot::new(project.layout.deployed_dir(), modes))
}

/// The in-memory DAG alterations, in the order the Python framework applied them.
///
/// # Errors
///
/// [`WiringError::Invalid`] when a configured default argument has no Python literal form.
pub fn dag_pipeline(project: &Project) -> Result<DagAlterationPipeline> {
    let mut transformers: Vec<Arc<dyn DagTransformer>> = Vec::new();
    for arg in &project.config.dags.default_args {
        let value = PyLiteral::from_json(&arg.value).ok_or_else(|| {
            WiringError::Invalid(format!(
                "vaultspeed.dags.default_args `{}` has no Python literal form",
                arg.key
            ))
        })?;
        transformers.push(Arc::new(DefaultArgsEntry::new(arg.key.clone(), value)));
    }
    transformers.push(Arc::new(EnsureImport::new(
        "airflow.datasets",
        "Dataset",
        "airflow",
    )));
    transformers.push(Arc::new(MappingTaskOutlets::new(
        project.config.dags.mapping_outlet_prefixes.clone(),
    )));
    transformers.push(Arc::new(LoadSuccessOutlet::new(SourceResolver::new(
        project.fmc_sources.clone(),
    ))));
    Ok(DagAlterationPipeline::new(
        transformers,
        vec![Arc::new(SyntaxValidator)],
    ))
}

/// The context the DAG transformers need for `environment`.
///
/// # Errors
///
/// [`WiringError::Invalid`] when no dataset token is configured and none can be derived from `singlestore_host`.
pub fn dag_context(
    project: &Project,
    environment: Environment,
    singlestore_host: &str,
) -> Result<DagContext> {
    let env = project.config.environment(environment);
    let dataset_env = match env.dataset_env() {
        Some(token) => token.to_owned(),
        None => derive_dataset_env(singlestore_host).ok_or_else(|| {
            WiringError::Invalid(format!(
                "cannot derive the dataset environment from host `{singlestore_host}`; \
                 set vaultspeed.environments.{environment}.dataset_env"
            ))
        })?,
    };
    Ok(DagContext {
        environment,
        dataset_env,
        database: env.database.clone(),
        singlestore_host: singlestore_host.to_owned(),
        singlestore_port: env.singlestore_port,
    })
}

/// `singlestore-rel1ldv.example.com` → `rel1ldv` (the Python convention).
fn derive_dataset_env(host: &str) -> Option<String> {
    let start = host.find("rel1")?;
    Some(
        host[start..]
            .split('.')
            .next()
            .unwrap_or_default()
            .to_owned(),
    )
}

/// The SingleStore connection of `environment` from the secrets (`SINGLESTORE_HOST`/`SERVER`/`URL`,
/// `SINGLESTORE_USER`/`USERNAME`, `SINGLESTORE_PASS`/`PASSWORD`/`PWD`). A configured CA means TLS with
/// the chain and the host name checked.
///
/// # Errors
///
/// [`WiringError::MissingSecret`] when the host, user or password secret is missing; [`WiringError::Invalid`] when the
/// settings are otherwise invalid (for example a conflicting pair of synonyms).
pub fn connection_settings(
    project: &Project,
    environment: Environment,
    secrets: &dyn SecretProvider,
) -> Result<ConnectionSettings> {
    let env = project.config.environment(environment);
    let mut builder = ConnectionSettings::from_secrets(secrets)
        .prefix("SINGLESTORE_")
        .port(env.singlestore_port)
        .database(env.database.clone())
        .connect_timeout(Duration::from_secs(30));
    if let Some(ca) = env.singlestore_ca_certificate() {
        builder = builder
            .ca_certificate(project.asset(Path::new(ca)))
            .tls(TlsMode::VerifyFull);
    }
    builder.build().map_err(|err| match err {
        vdp_db::SettingsError::MissingRequired { keys, .. } => WiringError::MissingSecret(keys),
        other => WiringError::Invalid(vdp_core::display_chain(&other)),
    })
}

/// The secrets that describe the Airflow host for DAG uploads (`vdp/airflow` in Vault).
pub fn airflow_secret_names(project: &Project) -> SecretNames {
    let dags = &project.config.dags;
    SecretNames {
        host: vec!["AIRFLOW_URL".into(), "AIRFLOW_HOST".into()],
        user: Vec::new(),
        key: vec!["private_key".into(), "AIRFLOW_PRIVATE_KEY".into()],
        default_user: Some(dags.ssh_user.clone()),
        default_port: dags.ssh_port,
        known_hosts: dags.known_hosts.clone(),
    }
}

/// The `SingleStore` host for dataset URIs when there is no connection (offline previews): the secret when known,
/// else a placeholder that still derives the environment's dataset token.
pub fn placeholder_host(environment: Environment, secrets: Option<&dyn SecretProvider>) -> String {
    secrets
        .and_then(|s| first_of(s, &["SINGLESTORE_HOST"]))
        .map_or_else(
            || {
                tracing::warn!(
                    "SINGLESTORE_HOST unknown offline; using a placeholder in dataset URIs"
                );
                format!("singlestore-rel1{}.invalid", environment.lower())
            },
            |h| h.expose_secret().to_owned(),
        )
}

/// Options for [`ledger`].
#[derive(Debug, Clone, Copy)]
pub struct LedgerOptions<'a> {
    pub environment: Environment,
    pub session: Option<&'a Arc<dyn SqlSession>>,
    pub mode: RunMode,
}

/// The ledger for an environment, per configuration. In preview the ledger is a
/// read-only in-memory snapshot: nothing (no table, no file) is created or written.
///
/// # Errors
///
/// [`WiringError::Ledger`] when the ledger cannot be opened or read (the table, the JSONL file, or an invalid table name).
pub fn ledger(project: &Project, options: LedgerOptions<'_>) -> Result<Arc<dyn Ledger>> {
    let LedgerOptions {
        environment,
        session,
        mode,
    } = options;
    let settings = &project.config.ledger;
    let jsonl_path = project.jsonl_ledger_path(environment);
    // Entries recorded before names were database-qualified are matched against the environment's database.
    let default_database = project.config.environment(environment).database;
    let use_database = settings.backend == LedgerBackend::Database && session.is_some();
    if mode.is_preview() {
        return Ok(match session.filter(|_| use_database) {
            Some(session) => Arc::new(SqlLedger::snapshot_in(
                session.as_ref(),
                &LedgerTable::new(&settings.schema, &settings.table)?,
                &default_database,
            )?),
            None => Arc::new(JsonlLedger::snapshot_in(&jsonl_path, &default_database)?),
        });
    }
    let jsonl = || -> Result<Arc<JsonlLedger>> {
        Ok(Arc::new(JsonlLedger::open_in(
            &jsonl_path,
            &default_database,
        )?))
    };
    match session.filter(|_| use_database) {
        None => Ok(jsonl()?),
        Some(session) => {
            let table = LedgerTable::new(&settings.schema, &settings.table)?;
            let primary = Arc::new(SqlLedger::open_in(
                session.clone(),
                table,
                &default_database,
            )?);
            let mirrors: Vec<Arc<dyn LedgerSink>> = if settings.mirror_jsonl {
                vec![jsonl()?]
            } else {
                Vec::new()
            };
            Ok(Arc::new(CompositeLedger::new(primary, mirrors)))
        }
    }
}

/// Infrastructure resolved for a run.
#[derive(Debug)]
pub struct Infrastructure {
    pub session: Option<Arc<dyn SqlSession>>,
    pub ledger: Arc<dyn Ledger>,
    pub singlestore_host: String,
    /// Uploads DAGs to the Airflow host; `None`: DAGs are rendered into the repository only.
    pub airflow: Option<Arc<dyn RemoteTransport>>,
}

impl Infrastructure {
    /// No database, Vault or Airflow: the JSONL ledger (read-only in preview) and a placeholder host.
    ///
    /// # Errors
    ///
    /// [`WiringError::Ledger`] when the ledger cannot be read.
    pub fn offline(project: &Project, environment: Environment, mode: RunMode) -> Result<Self> {
        Ok(Self {
            session: None,
            ledger: ledger(
                project,
                LedgerOptions {
                    environment,
                    session: None,
                    mode,
                },
            )?,
            singlestore_host: placeholder_host(environment, None),
            airflow: None,
        })
    }
}

/// Options of a deploy/plan invocation.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub environment: Environment,
    pub mode: RunMode,
    pub only: Option<Vec<String>>,
    pub git_sha: Option<String>,
}

/// Inputs for [`engine_with_history`].
#[derive(Debug)]
pub struct EngineInputs<'a> {
    pub options: &'a RunOptions,
    pub infra: &'a Infrastructure,
    pub history: Arc<dyn ChangeRecorder>,
}

/// Wires the engine for one run, without an object history.
///
/// # Errors
///
/// As [`engine_with_history`].
pub fn engine(project: &Project, options: &RunOptions, infra: &Infrastructure) -> Result<Engine> {
    engine_with_history(
        project,
        EngineInputs {
            options,
            infra,
            history: Arc::new(NoHistory),
        },
    )
}

/// Wires the engine for one run; the plan runner reports every statement to `inputs.history`. Preview runs
/// execute nothing and always use [`NoHistory`], whatever is passed.
///
/// # Errors
///
/// [`WiringError::Invalid`] when applying without a database session, or a DAG setting is invalid;
/// [`WiringError::Rules`] when a rewrite-rule file cannot be read; [`WiringError::Ledger`] when the ledger cannot be read.
pub fn engine_with_history(project: &Project, inputs: EngineInputs<'_>) -> Result<Engine> {
    let EngineInputs {
        options,
        infra,
        history,
    } = inputs;
    let config = &project.config;
    let preview = options.mode.is_preview();
    let history: Arc<dyn ChangeRecorder> = if preview {
        Arc::new(NoHistory)
    } else {
        history
    };
    let pipelines = pipelines_for(project, options.environment)?;

    let probe: Option<Arc<dyn CatalogProbe>> = infra
        .session
        .as_ref()
        .map(|s| Arc::new(InformationSchemaProbe::new(s.clone())) as Arc<dyn CatalogProbe>);

    // Preview writes go to an in-memory overlay so later sources see earlier "would-run"s.
    let ledger: Arc<dyn Ledger> = if preview {
        Arc::new(OverlayLedger::new(infra.ledger.clone())?)
    } else {
        infra.ledger.clone()
    };
    let executor: Arc<dyn StatementExecutor> = match (&infra.session, preview) {
        (Some(session), false) => Arc::new(SessionExecutor::new(session.clone())),
        (None, false) => {
            return Err(WiringError::Invalid(
                "applying changes requires a database session".into(),
            ));
        }
        (_, true) => Arc::new(DryRunExecutor),
    };
    let classifier: Arc<dyn ErrorClassifier> = if config.planning.classify_errors {
        Arc::new(MySqlErrorClassifier)
    } else {
        Arc::new(NoClassification)
    };

    let mut zips = ReleaseZipDiscovery::new(project.layout.release_dir());
    if let Some(only) = &options.only {
        zips = zips.only(only.clone());
    }
    let discoveries: Vec<Arc<dyn SourceDiscovery>> = vec![
        Arc::new(PmlDiscovery::new(project.layout.pml_dir())),
        Arc::new(zips),
    ];
    let lifecycles: Vec<Arc<dyn SourceLifecycle>> = vec![
        Arc::new(PmlLifecycle::new(project.layout.deployed_dir().join("PML"))),
        Arc::new(ZipArchiveLifecycle::new(
            project.layout.archive_dir(),
            config.failure.archive_on_failure,
        )),
    ];

    let dags = dag_stage(project, options, infra)?;

    Ok(Engine::new(EngineParts {
        environment: options.environment,
        mode: options.mode,
        context: RunContext {
            run_id: RunId::generate(),
            environment: options.environment.to_string(),
            git_sha: options.git_sha.clone(),
        },
        discoveries,
        handlers: handlers(config, &pipelines),
        lifecycles,
        planner: planner(config, probe),
        runner: PlanRunner::new(PlanRunnerParts {
            executor,
            classifier,
            ledger: ledger.clone(),
            skip_dependents: config.failure.skip_dependents,
        })
        .with_history(history),
        ledger,
        failure: failure_policies(config),
        snapshot: snapshot(project),
        dags: Some(dags),
    }))
}

fn dag_stage(project: &Project, options: &RunOptions, infra: &Infrastructure) -> Result<DagStage> {
    let config = &project.config;
    let preview = options.mode.is_preview();
    let mut publish_kinds = vec![GenerationKind::Fmc];
    if config.dags.publish_comp_dags {
        publish_kinds.push(GenerationKind::Comp);
    }
    let mut publishers: Vec<Arc<dyn DagPublisher>> = vec![Arc::new(RepoDirPublisher::new(
        project.layout.airflow_dags_dir(),
        project.layout.airflow_meta_dir(),
    ))];
    if let Some(transport) = &infra.airflow {
        publishers.push(Arc::new(SftpPublisher::new(
            transport.clone(),
            config.dags.remote_dags_dir.clone(),
            config.dags.remote_meta_dir.clone(),
        )));
    } else if !preview {
        tracing::warn!(
            "no AIRFLOW_URL/AIRFLOW_HOST secret: DAGs are rendered into the repository only"
        );
    }
    Ok(DagStage::new(DagStageParts {
        pipeline: dag_pipeline(project)?,
        namer: Arc::new(VaultSpeedDagNamer),
        publisher: Arc::new(CompositePublisher::new(publishers)),
        context: dag_context(project, options.environment, &infra.singlestore_host)?,
        publish_kinds,
    }))
}

/// Optional outputs and verbosity for [`report_sinks`].
#[derive(Debug, Clone)]
pub struct ReportSinkOptions {
    pub explain: bool,
    pub report_out: Option<PathBuf>,
    pub junit: Option<PathBuf>,
}

/// Standard report sinks for a run: console stdout, JSON and error CSV in the project's logs directory,
/// plus optional `--report-out` JSON and `--junit` XML file paths.
#[must_use]
pub fn report_sinks(project: &Project, options: ReportSinkOptions) -> Vec<Box<dyn ReportSink>> {
    let mut sinks: Vec<Box<dyn ReportSink>> = vec![
        Box::new(ConsoleSink::stdout(options.explain)),
        Box::new(JsonReportSink::in_dir(project.root.join("logs"))),
        Box::new(ErrorCsvSink::new(project.layout.logs_dir())),
    ];
    if let Some(path) = options.report_out {
        sinks.push(Box::new(JsonReportSink::to_file(path)));
    }
    if let Some(path) = options.junit {
        sinks.push(Box::new(JunitSink::new(path)));
    }
    sinks
}

#[cfg(test)]
mod tests {
    use vdp_secrets::{ChainProvider, MapProvider};

    use super::*;

    #[test]
    fn dataset_env_follows_the_python_convention() {
        assert_eq!(
            derive_dataset_env("singlestore-rel1ldv.example.com").as_deref(),
            Some("rel1ldv")
        );
        assert_eq!(derive_dataset_env("singlestore.example.com"), None);
        assert_eq!(derive_dataset_env("localhost"), None);
    }

    #[test]
    fn connection_settings_matches_golden_layout_with_and_without_ca_and_detects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let config = VaultspeedSettings::default();
        let project = Project::with_config(dir.path(), config).unwrap();
        let secrets = MapProvider::new("test")
            .with("SINGLESTORE_HOST", "s2.example")
            .with("SINGLESTORE_USER", "admin")
            .with("SINGLESTORE_PASS", "s3cr3t");

        let with_ca = connection_settings(&project, Environment::LDV, &secrets).unwrap();
        assert_eq!(with_ca.host, "s2.example");
        assert_eq!(with_ca.port, 3306);
        assert_eq!(with_ca.user, "admin");
        assert_eq!(with_ca.password.as_ref().unwrap().expose_secret(), "s3cr3t");
        assert_eq!(with_ca.database.as_deref(), Some("edw_dv_rdv"));
        assert_eq!(with_ca.tls, TlsMode::VerifyFull);
        assert_eq!(
            with_ca.ca_certificate.as_deref(),
            Some(
                dir.path()
                    .join("assets/root_intermediate_nonprod.cert")
                    .as_path()
            )
        );
        assert_eq!(with_ca.connect_timeout, Duration::from_secs(30));
        assert_eq!(with_ca.endpoint(), "admin@s2.example:3306/edw_dv_rdv");
        let dbg = format!("{with_ca:?}");
        assert!(!dbg.contains("s3cr3t"), "password leaked in Debug: {dbg}");

        let mut no_ca_config = VaultspeedSettings::default();
        no_ca_config
            .environments
            .get_mut(&Environment::LDV)
            .unwrap()
            .singlestore_ca_certificate = String::new();
        let no_ca_project = Project::with_config(dir.path(), no_ca_config).unwrap();
        let alt_secrets = MapProvider::new("test")
            .with("SINGLESTORE_HOST", "s2.example")
            .with("SINGLESTORE_USER", "admin")
            .with("SINGLESTORE_PASSWORD", "s3cr3t");
        let without_ca =
            connection_settings(&no_ca_project, Environment::LDV, &alt_secrets).unwrap();
        assert_eq!(without_ca.tls, TlsMode::Disabled);
        assert_eq!(without_ca.ca_certificate, None);
        assert_eq!(without_ca.endpoint(), "admin@s2.example:3306/edw_dv_rdv");

        let conflict = MapProvider::new("test")
            .with("SINGLESTORE_HOST", "s2.example")
            .with("SINGLESTORE_USER", "admin")
            .with("SINGLESTORE_PASS", "one")
            .with("SINGLESTORE_PASSWORD", "two");
        let err = connection_settings(&project, Environment::LDV, &conflict).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("SINGLESTORE_PASS") && msg.contains("SINGLESTORE_PASSWORD"),
            "{msg}"
        );

        let with_db_port_env = MapProvider::new("env")
            .with("SINGLESTORE_HOST", "s2.example")
            .with("SINGLESTORE_USER", "admin")
            .with("SINGLESTORE_PASS", "pw")
            .with("SINGLESTORE_DB", "wrong_db")
            .with("SINGLESTORE_DATABASE", "wrong_db2")
            .with("SINGLESTORE_PORT", "1");
        let preserved = connection_settings(&project, Environment::LDV, &with_db_port_env).unwrap();
        assert_eq!(preserved.database.as_deref(), Some("edw_dv_rdv"));
        assert_eq!(preserved.port, 3306);

        let make_env = || {
            MapProvider::new("env")
                .with("SINGLESTORE_HOST", "s2.example")
                .with("SINGLESTORE_USER", "admin")
                .with("SINGLESTORE_PASS", "from-env")
        };
        let make_vault = || {
            MapProvider::new("vault")
                .with("SINGLESTORE_PASSWORD", "from-vault")
                .with("SINGLESTORE_DB", "vault_db")
        };
        for layers in [
            vec![
                Box::new(make_env()) as Box<dyn SecretProvider>,
                Box::new(make_vault()),
            ],
            vec![
                Box::new(make_vault()) as Box<dyn SecretProvider>,
                Box::new(make_env()),
            ],
        ] {
            let chain = ChainProvider::new(layers);
            let resolved = connection_settings(&project, Environment::LDV, &chain).unwrap();
            assert_eq!(
                resolved.password.as_ref().unwrap().expose_secret(),
                "from-env",
                "expected first_of order (PASS before PASSWORD) across layers"
            );
            assert_eq!(resolved.database.as_deref(), Some("edw_dv_rdv"));
        }
    }

    #[test]
    fn report_sinks_includes_default_and_optional_file_sinks() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::with_config(dir.path(), VaultspeedSettings::default()).unwrap();
        assert_eq!(
            report_sinks(
                &project,
                ReportSinkOptions {
                    explain: false,
                    report_out: None,
                    junit: None,
                },
            )
            .len(),
            3
        );

        let report_out = dir.path().join("custom/report.json");
        let junit_out = dir.path().join("custom/junit.xml");
        let sinks = report_sinks(
            &project,
            ReportSinkOptions {
                explain: false,
                report_out: Some(report_out.clone()),
                junit: Some(junit_out.clone()),
            },
        );
        assert_eq!(sinks.len(), 5);

        let report = vdp_vaultspeed_domain::RunReport {
            run_id: RunId::from_string("run-test"),
            environment: Environment::LDV,
            mode: RunMode::Preview,
            started_at: chrono::Utc::now(),
            finished_at: None,
            sources: Vec::new(),
        };
        for sink in &sinks {
            sink.write(&report).unwrap();
        }
        assert!(report_out.is_file());
        assert!(junit_out.is_file());
    }
}
