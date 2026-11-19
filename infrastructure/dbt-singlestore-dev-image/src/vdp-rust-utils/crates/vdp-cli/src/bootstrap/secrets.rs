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

    fn layers(&self) -> Vec<&dyn SecretProvider> {
        self.chain.layers()
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
    let env_short = ctx.environment().short_name();
    sources
        .precedence
        .iter()
        .map(|entry| match entry.trim() {
            "dotenv" => Ok(Source::Dotenv),
            "env" => Ok(Source::Env),
            other => other
                .strip_prefix("vault:")
                .map(|path| {
                    Source::Vault(
                        path.trim()
                            .replace("{env}", &env)
                            .replace("{env_short}", &env_short),
                    )
                })
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
///
/// # Errors
///
/// [`ServiceError`] when a configured secret source is unknown or its dotenv file cannot be read, or when Vault cannot
/// be read and `[secrets].vault_required` is set. Without that setting a Vault failure only warns.
pub fn resolve_secrets(ctx: &Context, use_vault: bool) -> Result<ResolvedSecrets, ServiceError> {
    resolve_secrets_from(ctx, use_vault, &SecretSources::for_project(ctx))
}

/// [`resolve_secrets`] with a module's own sources.
///
/// # Errors
///
/// As [`resolve_secrets`].
pub fn resolve_secrets_from(
    ctx: &Context,
    use_vault: bool,
    origins: &SecretSources,
) -> Result<ResolvedSecrets, ServiceError> {
    resolve_secrets_with(ctx, use_vault, origins, |path| {
        DotenvProvider::load(path)
            .map_err(|e| ServiceError::new("secrets", vdp_core::display_chain(&e)))
    })
}

fn resolve_secrets_with(
    ctx: &Context,
    use_vault: bool,
    origins: &SecretSources,
    mut load_dotenv_file: impl FnMut(&Path) -> Result<DotenvProvider, ServiceError>,
) -> Result<ResolvedSecrets, ServiceError> {
    let sources = parse_sources(ctx, origins)?;
    let mut cached_dotenv: Option<DotenvProvider> = None;
    let mut load_dotenv = || -> Result<DotenvProvider, ServiceError> {
        if let Some(provider) = &cached_dotenv {
            return Ok(provider.clone());
        }
        let provider = match &origins.dotenv {
            Some(path) => load_dotenv_file(path)?,
            None => DotenvProvider::empty(),
        };
        cached_dotenv = Some(provider.clone());
        Ok(provider)
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
    let client = VaultClient::login_approle(&vdp_secrets::AppRoleCredentials {
        url: &settings.vault_url,
        role_id: &role_id,
        secret_id: &secret_id,
        ca_certificate: ca.as_deref(),
    })
    .map_err(|e| VaultFailure::Failed(vdp_core::display_chain(&e)))?;
    let mount = &ctx.config().secrets.vault_mount;
    let mut providers = Vec::with_capacity(paths.len());
    for path in paths {
        let provider = VaultKvProvider::load(&client, mount, std::slice::from_ref(path))
            .map_err(|e| VaultFailure::Failed(vdp_core::display_chain(&e)))?;
        providers.push((path.clone(), Box::new(provider) as Box<dyn SecretProvider>));
    }
    client.revoke();
    Ok(providers)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use tiny_http::{Response, Server, StatusCode};
    use vdp_core::Config;
    use vdp_secrets::ExposeSecret;

    use super::*;

    struct FakeVault {
        url: String,
        calls: Arc<Mutex<Vec<(String, String)>>>,
        stop: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl FakeVault {
        fn start(kv_status: u16) -> Self {
            let server = Server::http("127.0.0.1:0").unwrap();
            let url = format!("http://{}", server.server_addr());
            let calls = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let calls_bg = Arc::clone(&calls);
            let stop_bg = Arc::clone(&stop);
            let handle = thread::spawn(move || {
                while !stop_bg.load(Ordering::Acquire) {
                    let Ok(Some(req)) = server.recv_timeout(Duration::from_millis(15)) else {
                        continue;
                    };
                    let method = req.method().as_str().to_owned();
                    let path = req.url().to_owned();
                    calls_bg
                        .lock()
                        .unwrap()
                        .push((method.clone(), path.clone()));
                    let (status, body) = match (method.as_str(), path.as_str()) {
                        ("POST", "/v1/auth/approle/login") => {
                            (200, r#"{"auth":{"client_token":"approle-tok"}}"#.to_owned())
                        }
                        ("GET", "/v1/kv-other/data/vdp/airflow") => (
                            kv_status,
                            if kv_status == 200 {
                                r#"{"data":{"data":{"FOO":"bar"}}}"#.to_owned()
                            } else {
                                r#"{"errors":["boom"]}"#.to_owned()
                            },
                        ),
                        ("POST", "/v1/auth/token/revoke-self") => (204, String::new()),
                        _ => (404, String::new()),
                    };
                    let _ = req
                        .respond(Response::from_string(body).with_status_code(StatusCode(status)));
                }
            });
            Self {
                url,
                calls,
                stop,
                handle: Some(handle),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        fn revoke_count(&self) -> usize {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, u)| m == "POST" && u == "/v1/auth/token/revoke-self")
                .count()
        }
    }

    impl Drop for FakeVault {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    #[test]
    fn dotenv_file_is_read_once_when_vault_and_dotenv_are_both_configured() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        std::fs::write(&dotenv, "MY_SECRET=hello\n").unwrap();

        let ctx = Context::builder(Config::default()).build();
        let origins = SecretSources {
            origin: "[secrets].precedence",
            precedence: vec!["vault:vdp/airflow".into(), "dotenv".into()],
            dotenv: Some(dotenv),
            assets_dir: dir.path().to_path_buf(),
        };
        let reads = AtomicUsize::new(0);
        let resolved = resolve_secrets_with(&ctx, true, &origins, |path| {
            reads.fetch_add(1, Ordering::SeqCst);
            DotenvProvider::load(path)
                .map_err(|e| ServiceError::new("secrets", vdp_core::display_chain(&e)))
        })
        .unwrap();

        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(resolved.get("MY_SECRET").unwrap().expose_secret(), "hello");
    }

    #[test]
    fn absent_dotenv_uses_empty_provider_without_disk_reads() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context::builder(Config::default()).build();
        let origins = SecretSources {
            origin: "[secrets].precedence",
            precedence: vec!["vault:vdp/airflow".into(), "dotenv".into()],
            dotenv: None,
            assets_dir: dir.path().to_path_buf(),
        };
        let reads = AtomicUsize::new(0);
        let resolved = resolve_secrets_with(&ctx, true, &origins, |path| {
            reads.fetch_add(1, Ordering::SeqCst);
            DotenvProvider::load(path)
                .map_err(|e| ServiceError::new("secrets", vdp_core::display_chain(&e)))
        })
        .unwrap();

        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(resolved.chain.provider_names(), ["dotenv (none)"]);
    }

    #[test]
    fn bootstrap_error_after_login_still_revokes_vault_token_and_vault_token_without_role_id_skips()
    {
        let run_case = |kv_status: u16, dotenv_contents: &str| -> (ResolvedSecrets, usize, usize) {
            let fake = FakeVault::start(kv_status);
            let dir = tempfile::tempdir().unwrap();
            let dotenv = dir.path().join(".env");
            std::fs::write(&dotenv, dotenv_contents).unwrap();
            let toml = format!(
                "[environments.LDV]\nvault_url = \"{}\"\nca_certificate = \"\"\n",
                fake.url
            );
            let config =
                Config::from_documents(vec![("test.toml".into(), toml.parse().unwrap())]).unwrap();
            let ctx = Context::builder(config).build();
            let origins = SecretSources {
                origin: "[secrets].precedence",
                precedence: vec!["vault:vdp/airflow".into(), "dotenv".into()],
                dotenv: Some(dotenv),
                assets_dir: dir.path().to_path_buf(),
            };
            let resolved = resolve_secrets_from(&ctx, true, &origins).unwrap();
            (resolved, fake.revoke_count(), fake.call_count())
        };

        // 1. Login followed by `?` error when reading KV: token is still revoked via Drop.
        let (resolved, revokes, _) = run_case(500, "KEYVAULT_ROLE_ID=r\nKEYVAULT_SECRET_ID=s\n");
        assert!(matches!(resolved.vault, VaultStatus::Unavailable(_)));
        assert_eq!(revokes, 1);

        // 2. VAULT_TOKEN without AppRole role id: skipped with 0 Vault requests.
        let (resolved, revokes, calls) = run_case(200, "VAULT_TOKEN=dummy\n");
        assert_eq!(
            resolved.vault,
            VaultStatus::Skipped(
                "no AppRole role id (KEYVAULT_ROLE_ID or KEYVAULT_ROLE_ID_LDV_ABT)".into()
            )
        );
        assert_eq!(revokes, 0);
        assert_eq!(calls, 0);
    }

    /// `{env}` is the lower-case canonical name (`ldv`), `{env_short}` the short one (`dev`), also for the `dev` alias.
    #[test]
    fn vault_paths_render_env_and_env_short() {
        let sources = SecretSources {
            origin: "test",
            precedence: vec![
                "vault:vdp/{env}/dbt".into(),
                "vault:vdp/{env_short}/dbt".into(),
            ],
            dotenv: None,
            assets_dir: PathBuf::new(),
        };
        let paths = |name: &str| -> Vec<String> {
            let ctx = Context::builder(Config::default())
                .environment(vdp_core::Environment::named(name).unwrap())
                .build();
            parse_sources(&ctx, &sources)
                .unwrap()
                .into_iter()
                .filter_map(|s| match s {
                    Source::Vault(path) => Some(path),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(paths("dev"), ["vdp/ldv/dbt", "vdp/dev/dbt"]);
        assert_eq!(paths("PRD"), ["vdp/prd/dbt", "vdp/prd/dbt"]);
        assert_eq!(paths("cbt"), ["vdp/cbt/dbt", "vdp/cbt/dbt"]);
    }
}
