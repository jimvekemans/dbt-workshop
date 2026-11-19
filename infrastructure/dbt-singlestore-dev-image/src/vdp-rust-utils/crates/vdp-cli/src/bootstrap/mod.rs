//! Wiring: configuration discovery, the environment, the project and the services every module may ask for.

mod secrets;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use vdp_core::{Config, Context, Environment, ProjectLayout, ServiceError};
use vdp_db::{AdbcSession, DriverSource, SqlSession};
use vdp_dbt::profile::{self, ResolvedTarget};
use vdp_dbt::{DbtEngine, DbtProject, DbtSettings};
use vdp_secrets::{ExposeSecret, SecretProvider};

pub use secrets::{
    ResolvedSecrets, SecretSources, VaultStatus, resolve_secrets, resolve_secrets_from,
};

use crate::args::GlobalArgs;

/// System-wide configuration (the image can ship one).
pub const SYSTEM_CONFIG: &str = "/etc/vdp/vdp.toml";
/// Per-project configuration, next to `dbt_project.yml`.
pub const PROJECT_CONFIG: &str = "vdp.toml";

/// Logs go to stderr; `VDP_LOG` overrides the default filter.
pub fn init_logging(verbose: bool, default_filter: &str) {
    let filter = if verbose { "debug" } else { default_filter };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("VDP_LOG")
                .unwrap_or_else(|_| filter.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .without_time()
        .try_init();
}

/// The configuration files that apply, lowest priority first, with whether each must exist.
pub fn config_files(
    project: Option<&ProjectLayout>,
    explicit: Option<&Path>,
) -> Vec<(PathBuf, bool)> {
    let mut files = vec![(PathBuf::from(SYSTEM_CONFIG), false)];
    if let Some(project) = project {
        files.push((project.root.join(PROJECT_CONFIG), false));
    }
    if let Some(explicit) = explicit {
        files.push((explicit.to_owned(), true));
    }
    files
}

/// The project for `--project-dir`, or the nearest one above the current directory.
pub fn locate_project(project_dir: Option<&Path>, cwd: &Path) -> Option<ProjectLayout> {
    match project_dir {
        Some(dir) => Some(ProjectLayout::at(&cwd.join(dir))),
        None => ProjectLayout::discover(cwd).ok(),
    }
}

/// `--env`, else the configured variable (`DBT_DEPLOY_TARGET`), else the configured default; it must be one of the
/// configured environments (the four built-in ones, plus any `[environments.<name>]`), spelled as configured.
pub fn select_environment(explicit: Option<Environment>, config: &Config) -> Result<Environment> {
    if let Some(env) = explicit {
        return Ok(config.known_environment(env)?);
    }
    match std::env::var(&config.core.environment_var) {
        Ok(value) if !value.trim().is_empty() => config
            .environment_named(&value)
            .with_context(|| format!("${}", config.core.environment_var)),
        _ => Ok(config.core.default_environment),
    }
}

/// The dbt target and profiles directory the toolkit's own dbt runs and connections use.
#[derive(Debug, Clone, Default)]
pub struct DbtSelection {
    pub target: Option<String>,
    pub profiles_dir: Option<PathBuf>,
}

impl DbtSelection {
    /// `args` plus `--target`/`--profiles-dir`, so dbt reads the same profile as the toolkit.
    pub fn args(&self, project_root: &Path, args: &[&str]) -> Vec<std::ffi::OsString> {
        let mut out: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
        let profiles = profile::profiles_file(project_root, self.profiles_dir.as_deref());
        if let Some(dir) = profiles.parent() {
            out.push("--profiles-dir".into());
            out.push(dir.into());
        }
        if let Some(target) = &self.target {
            out.push("--target".into());
            out.push(target.into());
        }
        out
    }
}

fn service(name: &'static str) -> impl Fn(String) -> ServiceError {
    move |message| ServiceError::new(name, message)
}

/// Where the SingleStore ADBC driver is loaded from: `$VDP_ADBC_DRIVER`, else `[core].adbc_driver`, else
/// `lib/dbt/` above the dbt executable or vdp itself (as dbt v2 finds it), else the library search path.
pub fn adbc_driver(ctx: &Context) -> DriverSource {
    let explicit = std::env::var_os("VDP_ADBC_DRIVER")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let configured = &ctx.config().core.adbc_driver;
            (!configured.as_os_str().is_empty()).then(|| ctx.cwd().join(configured))
        });
    let dbt = ctx
        .config()
        .section::<DbtSettings>(DbtSettings::SECTION)
        .ok()
        .and_then(|settings| vdp_dbt::locate_engine(Some(&settings.binary)).ok());
    let executables: Vec<PathBuf> = dbt
        .into_iter()
        .chain(std::env::current_exe().ok())
        .collect();
    let source = DriverSource::locate(explicit.as_deref(), &executables);
    tracing::debug!(driver = %source, "SingleStore ADBC driver");
    source
}

/// Builds the context with every service this build offers.
pub fn context(global: &GlobalArgs, cwd: &Path) -> Result<Context> {
    let project = locate_project(global.project_dir.as_deref(), cwd);
    let config = Config::load(&config_files(project.as_ref(), global.config.as_deref()))?;
    let environment = select_environment(global.environment, &config)?;
    let unknown = config.unknown_sections(crate::commands::MODULE_SECTIONS);
    if !unknown.is_empty() {
        tracing::warn!(sections = ?unknown, "configuration sections no module reads (typo?)");
    }
    let use_vault = !global.no_vault;
    let builder = Context::builder(config)
        .environment(environment)
        .project(project)
        .cwd(cwd.to_owned())
        .in_ci(std::env::var_os("CI_PROJECT_DIR").is_some())
        .provide(move |ctx: &Context| Ok(Arc::new(resolve_secrets(ctx, use_vault)?)))
        .provide(|ctx: &Context| {
            let resolved = ctx.service::<Arc<ResolvedSecrets>>()?;
            Ok(resolved as Arc<dyn SecretProvider>)
        })
        .provide_value(DbtSelection {
            target: global.target.clone(),
            profiles_dir: global.profiles_dir.as_ref().map(|d| cwd.join(d)),
        })
        .provide(|ctx: &Context| {
            let root = &ctx
                .project()
                .map_err(|e| ServiceError::new("dbt project", e))?
                .root;
            DbtProject::load(root)
                .map(Arc::new)
                .map_err(|e| ServiceError::new("dbt project", e))
        })
        .provide(|ctx: &Context| {
            let project = ctx.service::<Arc<DbtProject>>()?;
            let selection = ctx.service::<DbtSelection>()?;
            let secrets = ctx.service::<Arc<dyn SecretProvider>>()?;
            let lookup = |name: &str| {
                secrets
                    .get(name)
                    .map(|v| v.expose_secret().to_owned())
                    .or_else(|| std::env::var(name).ok())
            };
            profile::resolve(
                &project,
                selection.profiles_dir.as_deref(),
                selection.target.as_deref(),
                &lookup,
            )
            .map(Arc::new)
            .map_err(|e| ServiceError::new("dbt profile", e))
        })
        .provide(|ctx: &Context| {
            let target = ctx.service::<Arc<ResolvedTarget>>()?;
            let session = AdbcSession::connect(&adbc_driver(ctx), &target.settings)
                .map_err(|e| service("database")(format!("{}: {e}", target.settings.endpoint())))?;
            Ok(Arc::new(session) as Arc<dyn SqlSession>)
        })
        .provide(|ctx: &Context| {
            let settings: DbtSettings = ctx
                .config()
                .section(DbtSettings::SECTION)
                .map_err(|e| ServiceError::new("dbt", e))?;
            let binary = vdp_dbt::locate_engine(Some(&settings.binary))
                .map_err(|e| ServiceError::new("dbt", e))?;
            let secrets = ctx.service::<Arc<dyn SecretProvider>>().ok();
            vdp_dbt::wrapper::module_engine(ctx, binary, secrets.as_deref())
                .map(|engine| Arc::new(engine) as Arc<dyn DbtEngine>)
                .map_err(|e| ServiceError::new("dbt", e))
        });
    #[cfg(feature = "release")]
    let builder = release_services(builder);
    Ok(builder.build())
}

/// Git and Jira, for the release module.
#[cfg(feature = "release")]
fn release_services(builder: vdp_core::ContextBuilder) -> vdp_core::ContextBuilder {
    builder
        .provide(|ctx: &Context| {
            // The repository of the dbt project, else the current directory.
            let root = ctx.project_opt().map_or_else(
                || ctx.cwd().to_owned(),
                |p| p.repo_root.clone().unwrap_or_else(|| p.root.clone()),
            );
            // The source branch (dev) is never written, not even locally; only release branches and main are.
            let release: vdp_release::ReleaseSettings = ctx
                .config()
                .section(vdp_release::ReleaseSettings::SECTION)
                .map_err(|e| ServiceError::new("git", e))?;
            let guard = vdp_release::BranchRules::from_settings(&release).guard();
            Ok(Arc::new(vdp_git::GitCli::new(&root).with_guard(guard)) as Arc<dyn vdp_git::Vcs>)
        })
        .provide(|ctx: &Context| {
            let settings: vdp_jira::JiraSettings = ctx
                .config()
                .section(vdp_jira::JiraSettings::SECTION)
                .map_err(|e| ServiceError::new("jira", e))?;
            let secrets = ctx.service::<Arc<dyn SecretProvider>>()?;
            let user = vdp_secrets::require(secrets.as_ref(), &[settings.user_secret.as_str()])
                .map_err(|e| ServiceError::new("jira", e))?;
            let token = vdp_secrets::require(secrets.as_ref(), &[settings.token_secret.as_str()])
                .map_err(|e| ServiceError::new("jira", e))?;
            let client = vdp_jira::JiraClient::new(&settings.base_url, &user, &token)
                .with_sprint_field(&settings.sprint_field);
            Ok(Arc::new(client) as Arc<dyn vdp_jira::IssueTracker>)
        })
}
