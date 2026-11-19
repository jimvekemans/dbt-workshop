//! Infrastructure for VaultSpeed deployments: the framework checkout, its secrets, the SingleStore session and
//! the Airflow transport. The engine itself is wired by `vdp_vaultspeed::wiring`.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use vdp_core::{Context, Environment};
use vdp_db::{AdbcSession, SqlSession};
use vdp_secrets::SecretProvider;
use vdp_ssh::{OpenSshTransport, RemoteTransport, SshAccess};
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed::wiring::{self, Infrastructure, Project};
use vdp_vaultspeed_domain::RunMode;

use vdp_cli::bootstrap::{ResolvedSecrets, SecretSources, adbc_driver, resolve_secrets_from};

/// The framework checkout: `--framework-dir`, else `[vaultspeed].project_dir`, relative to the current directory.
pub fn project(ctx: &Context, framework_dir: Option<&Path>) -> Result<Project> {
    let settings: VaultspeedSettings = ctx.config().section(VaultspeedSettings::SECTION)?;
    let root = ctx
        .cwd()
        .join(framework_dir.unwrap_or(&settings.project_dir));
    let root = root.canonicalize().unwrap_or(root);
    Project::with_config(&root, settings).context("[vaultspeed] settings")
}

/// The VaultSpeed secret chain: `[vaultspeed].secrets_precedence`, the framework's dotenv file and its CA
/// certificates; Vault (URL, AppRole, mount) as for every other module.
pub fn secrets(ctx: &Context, project: &Project, use_vault: bool) -> Result<ResolvedSecrets> {
    let sources = SecretSources {
        origin: "[vaultspeed].secrets_precedence",
        precedence: project.config.secrets_precedence.clone(),
        dotenv: Some(project.root.join(&project.config.paths.dotenv_file)),
        assets_dir: project.root.join(&project.config.paths.assets_dir),
    };
    Ok(resolve_secrets_from(ctx, use_vault, &sources)?)
}

/// What a run needs. The Airflow key file lives as long as this value.
#[derive(Debug)]
pub struct Resolved {
    pub infra: Infrastructure,
    _airflow: Option<SshAccess>,
}

/// Secrets, the database session, the ledger and Airflow access. Offline: no Vault, no database, no Airflow; the
/// JSONL ledger (read-only in preview).
pub fn infrastructure(
    ctx: &Context,
    project: &Project,
    environment: Environment,
    offline: bool,
    use_vault: bool,
    mode: RunMode,
) -> Result<Resolved> {
    let secrets = secrets(ctx, project, use_vault && !offline)?;
    let provider: &dyn SecretProvider = &secrets;
    if offline {
        return Ok(Resolved {
            infra: Infrastructure {
                session: None,
                ledger: wiring::ledger(project, environment, None, mode)?,
                singlestore_host: wiring::placeholder_host(environment, Some(provider)),
                airflow: None,
            },
            _airflow: None,
        });
    }
    let settings = wiring::connection_settings(project, environment, provider)?;
    let session: Arc<dyn SqlSession> = Arc::new(
        AdbcSession::connect(&adbc_driver(ctx), &settings)
            .with_context(|| format!("connecting to {}", settings.endpoint()))?,
    );
    let access = if mode.is_preview() {
        None
    } else {
        SshAccess::from_secrets(provider, &wiring::airflow_secret_names(project))?
    };
    let airflow = access
        .as_ref()
        .map(|a| Arc::new(OpenSshTransport::new(a.endpoint.clone())) as Arc<dyn RemoteTransport>);
    Ok(Resolved {
        infra: Infrastructure {
            ledger: wiring::ledger(project, environment, Some(&session), mode)?,
            session: Some(session),
            singlestore_host: settings.host.clone(),
            airflow,
        },
        _airflow: access,
    })
}
