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
use crate::engine::{DagStage, Engine, EngineParts, FailurePolicies};
use crate::executor::{
    DryRunExecutor, ErrorClassifier, FailurePolicy, InformationSchemaProbe, MySqlErrorClassifier,
    NoClassification, PlanRunner, RunContext, SessionExecutor, StatementExecutor,
};
use crate::ledger::{
    CompositeLedger, JsonlLedger, Ledger, LedgerSink, LedgerTable, OverlayLedger, SqlLedger,
};
use crate::planner::{
    AlwaysRunPolicy, CatalogProbe, DdlPhases, DowngradeGuard, EtlPhases, ExclusionPolicy,
    ExecutionGuard, IdempotencyPolicy, OnceOnlyPolicy, Planner, PmlPhases, RebuildGuard,
    ReplaceablePolicy, SequentialPhasing, StandaloneDrops,
};
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
    #[error("loading rewrite rules from {path}: {source}")]
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
pub fn pipelines(project: &Project) -> Result<Pipelines> {
    pipelines_in(project, "")
}

/// [`pipelines`] for one environment: unqualified object names in the SQL resolve to the environment's
/// database (`[vaultspeed.environments.<env>].database`), so they are the same objects as their qualified
/// spellings in the ledger.
pub fn pipelines_for(project: &Project, environment: Environment) -> Result<Pipelines> {
    pipelines_in(project, &project.config.environment(environment).database)
}

/// [`pipelines`] with `database` as the database in effect when a script has no `USE` (empty: unknown).
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

/// `singlestore.example.com` → `rel1ldv` (the Python convention).
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

fn require(secrets: &dyn SecretProvider, names: &[&str]) -> Result<String> {
    first_of(secrets, names)
        .map(|v| v.expose_secret().to_owned())
        .ok_or_else(|| WiringError::MissingSecret(names.join(" or ")))
}

/// The SingleStore connection of `environment` from the secrets (`SINGLESTORE_HOST`, `SINGLESTORE_USER`,
/// `SINGLESTORE_PASS`/`SINGLESTORE_PASSWORD`). A configured CA means TLS with the chain and the host name checked.
pub fn connection_settings(
    project: &Project,
    environment: Environment,
    secrets: &dyn SecretProvider,
) -> Result<ConnectionSettings> {
    let env = project.config.environment(environment);
    let mut settings = ConnectionSettings::new(
        require(secrets, &["SINGLESTORE_HOST"])?,
        env.singlestore_port,
        require(secrets, &["SINGLESTORE_USER"])?,
    );
    settings.password = Some(
        first_of(secrets, &["SINGLESTORE_PASS", "SINGLESTORE_PASSWORD"]).ok_or_else(|| {
            WiringError::MissingSecret("SINGLESTORE_PASS or SINGLESTORE_PASSWORD".into())
        })?,
    );
    settings.database = Some(env.database.clone());
    if let Some(ca) = env.singlestore_ca_certificate() {
        settings.ca_certificate = Some(project.asset(Path::new(ca)));
        settings.tls = TlsMode::VerifyFull;
    }
    settings.connect_timeout = Duration::from_secs(30);
    Ok(settings)
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

/// The ledger for an environment, per configuration. In preview the ledger is a
/// read-only in-memory snapshot: nothing (no table, no file) is created or written.
pub fn ledger(
    project: &Project,
    environment: Environment,
    session: Option<&Arc<dyn SqlSession>>,
    mode: RunMode,
) -> Result<Arc<dyn Ledger>> {
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
    pub fn offline(project: &Project, environment: Environment, mode: RunMode) -> Result<Self> {
        Ok(Self {
            session: None,
            ledger: ledger(project, environment, None, mode)?,
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

/// Wires the engine for one run, without an object history.
pub fn engine(project: &Project, options: &RunOptions, infra: &Infrastructure) -> Result<Engine> {
    engine_with_history(project, options, infra, Arc::new(NoHistory))
}

/// Wires the engine for one run; the plan runner reports every statement to `history`. Preview runs execute
/// nothing and always use [`NoHistory`], whatever is passed.
pub fn engine_with_history(
    project: &Project,
    options: &RunOptions,
    infra: &Infrastructure,
    history: Arc<dyn ChangeRecorder>,
) -> Result<Engine> {
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
    let dags = DagStage::new(
        dag_pipeline(project)?,
        Arc::new(VaultSpeedDagNamer),
        Arc::new(CompositePublisher::new(publishers)),
        dag_context(project, options.environment, &infra.singlestore_host)?,
        publish_kinds,
    );

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
        runner: PlanRunner::new(
            executor,
            classifier,
            ledger.clone(),
            config.failure.skip_dependents,
        )
        .with_history(history),
        ledger,
        failure: failure_policies(config),
        snapshot: snapshot(project),
        dags: Some(dags),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_env_follows_the_python_convention() {
        assert_eq!(
            derive_dataset_env("singlestore.example.com").as_deref(),
            Some("rel1ldv")
        );
        assert_eq!(derive_dataset_env("localhost"), None);
    }
}
