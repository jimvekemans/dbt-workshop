use std::fmt;
use std::path::{Path, PathBuf};

use vdp_core::{Context, ServiceError};
use vdp_secrets::{
    ChainProvider, DotenvProvider, EnvProvider, SecretProvider, SecretString, VaultClient,
    VaultKvProvider, first_of,
};

/// What happened with Vault while resolving secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultStatus {
    /// Not attempted (`--no-vault`, or no Vault source configured).
    Skipped(String),
    /// Read these paths.
    Read(Vec<String>),
    /// Attempted and failed; local secrets only.
    Unavailable(String),
}

/// The secret chain in configured precedence, plus how Vault went.
pub struct ResolvedSecrets {
    pub chain: ChainProvider,
    pub vault: VaultStatus,
}

impl fmt::Debug for ResolvedSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedSecrets")
            .field("sources", &self.chain.provider_names())
            .field("vault", &self.vault)
            .finish()
    }
}

impl SecretProvider for ResolvedSecrets {
    fn name(&self) -> String {
        self.chain.name()
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        self.chain.get(key)
    }

    fn keys(&self) -> Vec<String> {
        self.chain.keys()
    }
}

/// Where a module's secrets come from: the precedence list, the dotenv file and the directory with the CA
/// certificates. dbt modules use `[secrets].precedence`, `[core].dotenv` and `[core].assets_dir`; VaultSpeed
/// deployments their own (`[vaultspeed]`). Vault itself (URL, AppRole, CA name, mount) is shared.
#[derive(Debug, Clone)]
pub struct SecretSources {
    /// For messages: which setting the precedence came from.
    pub origin: &'static str,
    pub precedence: Vec<String>,
    pub dotenv: Option<PathBuf>,
    pub assets_dir: PathBuf,
}

impl SecretSources {
    /// The dbt project's sources.
    pub fn for_project(ctx: &Context) -> Self {
        Self {
            origin: "[secrets].precedence",
            precedence: ctx.config().secrets.precedence.clone(),
            dotenv: ctx
                .project_opt()
                .map(|p| p.resolve(&ctx.config().core.dotenv)),
            assets_dir: ctx.assets_dir(),
        }
    }
}

enum Source {
    Vault(String),
    Dotenv,
    Env,
}

fn parse_sources(ctx: &Context, sources: &SecretSources) -> Result<Vec<Source>, ServiceError> {
    let env = ctx.environment().lower();
    sources
        .precedence
        .iter()
        .map(|entry| match entry.trim() {
            "dotenv" => Ok(Source::Dotenv),
            "env" => Ok(Source::Env),
            other => other
                .strip_prefix("vault:")
                .map(|path| Source::Vault(path.trim().replace("{env}", &env)))
                .ok_or_else(|| {
                    ServiceError::new(
                        "secrets",
                        format!("unknown source '{other}' in {}", sources.origin),
                    )
                }),
        })
        .collect()
}

/// Resolves the secret chain for the context's environment.
///
/// Mirrors `ProjectManager.setup_env` of the Python utilities: the AppRole credentials come from the dotenv file
/// or the environment (`KEYVAULT_ROLE_ID`, else the environment-specific variable), Vault is read with the CA of
/// the environment, and a Vault failure only warns unless `[secrets].vault_required` is set.
pub fn resolve_secrets(ctx: &Context, use_vault: bool) -> Result<ResolvedSecrets, ServiceError> {
    resolve_secrets_from(ctx, use_vault, &SecretSources::for_project(ctx))
}

/// [`resolve_secrets`] with a module's own sources.
pub fn resolve_secrets_from(
    ctx: &Context,
    use_vault: bool,
    origins: &SecretSources,
) -> Result<ResolvedSecrets, ServiceError> {
    let sources = parse_sources(ctx, origins)?;
    let dotenv_path = origins.dotenv.clone();
    let load_dotenv = || -> Result<DotenvProvider, ServiceError> {
        match &dotenv_path {
            Some(path) => DotenvProvider::load(path).map_err(|e| ServiceError::new("secrets", e)),
            None => DotenvProvider::load(std::path::Path::new("/nonexistent/.env"))
                .map_err(|e| ServiceError::new("secrets", e)),
        }
    };

    let vault_paths: Vec<String> = sources
        .iter()
        .filter_map(|s| match s {
            Source::Vault(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    let (mut vault_providers, vault) = if vault_paths.is_empty() {
        (
            Vec::new(),
            VaultStatus::Skipped("no vault source configured".into()),
        )
    } else if !use_vault {
        (Vec::new(), VaultStatus::Skipped("--no-vault".into()))
    } else {
        let local = ChainProvider::new(vec![Box::new(load_dotenv()?), Box::new(EnvProvider)]);
        match read_vault(ctx, &local, &vault_paths, &origins.assets_dir) {
            Ok(providers) => (providers, VaultStatus::Read(vault_paths.clone())),
            Err(VaultFailure::NoCredentials(reason)) if !ctx.config().secrets.vault_required => {
                tracing::debug!(%reason, "Vault skipped");
                (Vec::new(), VaultStatus::Skipped(reason))
            }
            Err(VaultFailure::NoCredentials(reason) | VaultFailure::Failed(reason))
                if ctx.config().secrets.vault_required =>
            {
                return Err(ServiceError::new("vault", reason));
            }
            Err(VaultFailure::NoCredentials(reason) | VaultFailure::Failed(reason)) => {
                tracing::warn!(%reason, "cannot read secrets from Vault; using local secrets");
                (Vec::new(), VaultStatus::Unavailable(reason))
            }
        }
    };

    let mut chain = ChainProvider::default();
    for source in sources {
        match source {
            Source::Dotenv => chain.push(Box::new(load_dotenv()?)),
            Source::Env => chain.push(Box::new(EnvProvider)),
            Source::Vault(path) => {
                if let Some(index) = vault_providers.iter().position(|(p, _)| *p == path) {
                    chain.push(vault_providers.remove(index).1);
                }
            }
        }
    }
    Ok(ResolvedSecrets { chain, vault })
}

type VaultProviders = Vec<(String, Box<dyn SecretProvider>)>;

enum VaultFailure {
    /// No AppRole credentials anywhere: a developer without Vault access, not an error.
    NoCredentials(String),
    Failed(String),
}

fn names(vars: &[String]) -> Vec<&str> {
    vars.iter().map(String::as_str).collect()
}

fn read_vault(
    ctx: &Context,
    local: &dyn SecretProvider,
    paths: &[String],
    assets_dir: &Path,
) -> Result<VaultProviders, VaultFailure> {
    let settings = ctx.config().environment(ctx.environment());
    let missing = |what: &str, vars: &[String]| {
        VaultFailure::NoCredentials(format!("no AppRole {what} ({})", vars.join(" or ")))
    };
    let role_id = first_of(local, &names(&settings.role_id_vars))
        .ok_or_else(|| missing("role id", &settings.role_id_vars))?;
    let secret_id = first_of(local, &names(&settings.secret_id_vars))
        .ok_or_else(|| missing("secret id", &settings.secret_id_vars))?;
    let ca = assets_dir.join(&settings.ca_certificate);
    let ca = ca.is_file().then_some(ca);
    let client =
        VaultClient::login_approle(&settings.vault_url, &role_id, &secret_id, ca.as_deref())
            .map_err(|e| VaultFailure::Failed(e.to_string()))?;
    let mount = &ctx.config().secrets.vault_mount;
    let result = paths
        .iter()
        .map(|path| {
            VaultKvProvider::load(&client, mount, std::slice::from_ref(path))
                .map(|p| (path.clone(), Box::new(p) as Box<dyn SecretProvider>))
                .map_err(|e| VaultFailure::Failed(e.to_string()))
        })
        .collect();
    client.revoke();
    result
}
