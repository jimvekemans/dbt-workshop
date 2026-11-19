use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::{CoreError, Environment};

/// `[core]`: what every command needs before it can do anything else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CoreSettings {
    /// Environment used when neither `--env` nor `environment_var` says otherwise.
    pub default_environment: Environment,
    /// Variable naming the environment (the Python utilities read `DBT_DEPLOY_TARGET`).
    pub environment_var: String,
    /// Dotenv file, relative to the dbt project root.
    pub dotenv: PathBuf,
    /// Directory with the organisation's CA certificates. Empty: `/usr/app/dbt/assets` in CI (`CI_PROJECT_DIR` set),
    /// else `<project>/../assets`, falling back to `/usr/app/dbt/assets`.
    pub assets_dir: PathBuf,
    /// The SingleStore ADBC driver every database connection goes through. Empty: `$VDP_ADBC_DRIVER`, else
    /// `lib/dbt/libadbc_driver_singlestore.so` above the dbt executable or vdp itself, else the library path.
    pub adbc_driver: PathBuf,
}

impl Default for CoreSettings {
    fn default() -> Self {
        Self {
            default_environment: Environment::LDV,
            environment_var: "DBT_DEPLOY_TARGET".into(),
            dotenv: PathBuf::from(".devcontainer/.env"),
            assets_dir: PathBuf::new(),
            adbc_driver: PathBuf::new(),
        }
    }
}

impl CoreSettings {
    /// Resolves the assets directory for a project root (see [`CoreSettings::assets_dir`]).
    pub fn resolve_assets_dir(&self, project_root: Option<&Path>, in_ci: bool) -> PathBuf {
        const IMAGE_ASSETS: &str = "/usr/app/dbt/assets";
        if !self.assets_dir.as_os_str().is_empty() {
            return match project_root {
                Some(root) if self.assets_dir.is_relative() => root.join(&self.assets_dir),
                _ => self.assets_dir.clone(),
            };
        }
        let mut candidates = Vec::new();
        if in_ci {
            candidates.push(PathBuf::from(IMAGE_ASSETS));
        }
        if let Some(root) = project_root {
            candidates.push(root.join("..").join("assets"));
        }
        candidates.push(PathBuf::from(IMAGE_ASSETS));
        candidates
            .iter()
            .find(|c| c.is_dir())
            .unwrap_or(&candidates[0])
            .clone()
    }
}

/// `[secrets]`: where secret values come from and which source wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SecretsSettings {
    /// KV v2 mount of the VDP secrets.
    pub vault_mount: String,
    /// Sources, highest priority first: `vault:<path>` (`{env}` = lower-case environment, `ldv`; `{env_short}` = its
    /// short name, `dev`), `dotenv`, `env`.
    pub precedence: Vec<String>,
    /// Fail when Vault cannot be read. When false (the Python behaviour) the toolkit warns and goes on with
    /// local secrets.
    pub vault_required: bool,
}

impl Default for SecretsSettings {
    fn default() -> Self {
        Self {
            vault_mount: "kv-other".into(),
            // The Python utilities ended up with this precedence: Vault `vdp/<env>/dbt` was exported first, then
            // the dotenv file was loaded with override=True, then `vdp/airflow` was exported on top.
            precedence: vec![
                "vault:vdp/airflow".into(),
                "dotenv".into(),
                "vault:vdp/{env}/dbt".into(),
                "env".into(),
            ],
            vault_required: false,
        }
    }
}

/// `[environments.<ENV>]`: per-environment endpoints. The four built-in environments are always defined; a section
/// with another name (`[environments.dev]`) defines an environment of that name, starting from the non-production
/// defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EnvironmentSettings {
    pub vault_url: String,
    /// File name in the assets directory; the CA for Vault and for SingleStore TLS (`VDP_SSL_CA`).
    pub ca_certificate: String,
    /// Variables holding the AppRole role id; the first one that is set wins.
    pub role_id_vars: Vec<String>,
    /// Variables holding the AppRole secret id; the first one that is set wins.
    pub secret_id_vars: Vec<String>,
}

impl Default for EnvironmentSettings {
    fn default() -> Self {
        Self::non_production()
    }
}

impl EnvironmentSettings {
    /// LDV, ABT and CBT. The Python utilities treat CBT as non-production: ops Vault, non-prod CA and the
    /// `_LDV_ABT` AppRole, despite the `_CBT_PRD` variable names.
    pub fn non_production() -> Self {
        Self {
            vault_url: "https://vault.ops.example.com:8200".into(),
            ca_certificate: "root_intermediate_nonprod.cert".into(),
            role_id_vars: vec!["KEYVAULT_ROLE_ID".into(), "KEYVAULT_ROLE_ID_LDV_ABT".into()],
            secret_id_vars: vec![
                "KEYVAULT_SECRET_ID".into(),
                "KEYVAULT_SECRET_ID_LDV_ABT".into(),
            ],
        }
    }

    /// The defaults for the production Vault (`CBT` and `PRD` credentials).
    pub fn production() -> Self {
        Self {
            vault_url: "https://vault.example.com:8200".into(),
            ca_certificate: "root_intermediate_prod.cert".into(),
            role_id_vars: vec!["KEYVAULT_ROLE_ID".into(), "KEYVAULT_ROLE_ID_CBT_PRD".into()],
            secret_id_vars: vec![
                "KEYVAULT_SECRET_ID".into(),
                "KEYVAULT_SECRET_ID_CBT_PRD".into(),
            ],
        }
    }
}

fn default_environments() -> BTreeMap<Environment, EnvironmentSettings> {
    Environment::ALL
        .into_iter()
        .map(|env| {
            let settings = if env == Environment::PRD {
                EnvironmentSettings::production()
            } else {
                EnvironmentSettings::non_production()
            };
            (env, settings)
        })
        .collect()
}

/// The merged configuration document plus the core's own sections.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    document: Table,
    origins: Vec<String>,
    pub core: CoreSettings,
    pub secrets: SecretsSettings,
    pub environments: BTreeMap<Environment, EnvironmentSettings>,
}

impl Default for Config {
    fn default() -> Self {
        Self::from_documents(Vec::new()).expect("built-in defaults are valid")
    }
}

impl Config {
    /// Sections owned by the core.
    pub const CORE_SECTIONS: [&'static str; 3] = ["core", "secrets", "environments"];

    /// Loads and merges files in order (later files win key by key). Missing files are skipped unless they are
    /// `required`.
    ///
    /// # Errors
    ///
    /// [`CoreError::Read`] when a file exists (or is `required`) but cannot be read; [`CoreError::Config`] when a file is
    /// not valid TOML, a core section has the wrong shape, or `[core].default_environment` is not a defined environment.
    pub fn load(files: &[(PathBuf, bool)]) -> Result<Self, CoreError> {
        let mut documents = Vec::new();
        for (path, required) in files {
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    let table = text.parse::<Table>().map_err(|e| CoreError::Config {
                        origin: path.display().to_string(),
                        detail: e.to_string(),
                    })?;
                    documents.push((path.display().to_string(), table));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => {}
                Err(source) => {
                    return Err(CoreError::Read {
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
        Self::from_documents(documents)
    }

    /// Builds the configuration from parsed documents (`(origin, table)`), overlaid on the built-in defaults.
    ///
    /// # Errors
    ///
    /// [`CoreError::Config`] when the `[core]`, `[secrets]` or `[environments]` section has the wrong shape, or
    /// `[core].default_environment` is not a defined environment.
    pub fn from_documents(documents: Vec<(String, Table)>) -> Result<Self, CoreError> {
        let mut document = Self::core_defaults();
        let mut origins = vec!["built-in defaults".to_owned()];
        for (origin, mut table) in documents {
            canonical_environment_keys(&mut table);
            deep_merge(&mut document, table);
            origins.push(origin);
        }
        let origin = origins.join(" + ");
        let core: CoreSettings = Self::typed(&document, "core", &origin)?;
        let secrets = Self::typed(&document, "secrets", &origin)?;
        let environments = Self::typed(&document, "environments", &origin)?;
        let mut config = Self {
            document,
            origins,
            core,
            secrets,
            environments,
        };
        config.core.default_environment = config
            .known_environment(config.core.default_environment)
            .map_err(|e| CoreError::Config {
                origin,
                detail: format!("[core].default_environment: {e}"),
            })?;
        Ok(config)
    }

    /// The configured environment called `name` (case-insensitive), as the configuration spells it.
    ///
    /// # Errors
    ///
    /// [`CoreError::UnknownEnvironment`] when `name` is not a valid environment name or the configuration does not
    /// define it.
    pub fn environment_named(&self, name: &str) -> Result<Environment, CoreError> {
        self.known_environment(Environment::named(name)?)
    }

    /// `env` if the configuration defines it (case-insensitive), as the configuration spells it.
    ///
    /// # Errors
    ///
    /// [`CoreError::UnknownEnvironment`] when the configuration does not define `env`.
    pub fn known_environment(&self, env: Environment) -> Result<Environment, CoreError> {
        self.environments
            .keys()
            .find(|known| known.as_str().eq_ignore_ascii_case(env.as_str()))
            .copied()
            .ok_or_else(|| CoreError::UnknownEnvironment {
                name: env.as_str().to_owned(),
                known: self
                    .environments
                    .keys()
                    .map(|e| e.as_str().to_owned())
                    .collect(),
            })
    }

    fn core_defaults() -> Table {
        let mut table = Table::new();
        let to_value = |v: Result<Value, toml::ser::Error>| v.expect("defaults serialise");
        table.insert(
            "core".into(),
            to_value(Value::try_from(CoreSettings::default())),
        );
        table.insert(
            "secrets".into(),
            to_value(Value::try_from(SecretsSettings::default())),
        );
        let environments: BTreeMap<String, EnvironmentSettings> = default_environments()
            .into_iter()
            .map(|(k, v)| (k.as_str().to_owned(), v))
            .collect();
        table.insert(
            "environments".into(),
            to_value(Value::try_from(environments)),
        );
        table
    }

    fn typed<T: DeserializeOwned>(
        document: &Table,
        section: &str,
        origin: &str,
    ) -> Result<T, CoreError> {
        let value = document
            .get(section)
            .cloned()
            .unwrap_or_else(|| Value::Table(Table::new()));
        value
            .try_into()
            .map_err(|e: toml::de::Error| CoreError::Config {
                origin: origin.to_owned(),
                detail: format!("[{section}]: {e}"),
            })
    }

    /// A module's section, with the module's defaults for everything not configured.
    ///
    /// # Errors
    ///
    /// [`CoreError::Config`] when the section exists but does not deserialize into `T`.
    pub fn section<T: DeserializeOwned + Default>(&self, name: &str) -> Result<T, CoreError> {
        match self.document.get(name) {
            None => Ok(T::default()),
            Some(_) => Self::typed(&self.document, name, &self.origins.join(" + ")),
        }
    }

    /// Top-level sections nobody claims (typos, or a module this build lacks).
    pub fn unknown_sections(&self, known: &[&str]) -> Vec<String> {
        self.document
            .keys()
            .filter(|k| !Self::CORE_SECTIONS.contains(&k.as_str()) && !known.contains(&k.as_str()))
            .cloned()
            .collect()
    }

    /// Where the values came from, lowest priority first.
    pub fn origins(&self) -> &[String] {
        &self.origins
    }

    /// The Vault settings of `env`: the configured ones, else the production defaults for `PRD` and the non-production
    /// defaults for every other environment.
    pub fn environment(&self, env: Environment) -> EnvironmentSettings {
        self.environments.get(&env).cloned().unwrap_or_else(|| {
            if env == Environment::PRD {
                EnvironmentSettings::production()
            } else {
                EnvironmentSettings::non_production()
            }
        })
    }
}

/// `[environments.prd]` and `[environments.PRD]` are the same section, and so are `[environments.dev]` and
/// `[environments.LDV]` (`val` is `ABT`, see [`Environment::ALIASES`]): the built-in names are written in capitals
/// before merging, so an alias or lower-case section overrides the built-in defaults key by key instead of replacing
/// them.
///
/// When several spellings of one environment are present (`[environments.LDV]` and `[environments.dev]`, or `PRD` and
/// `prd`), they are merged key by key, the non-canonical spelling over the canonical one, so a conflicting value
/// comes from the non-canonical section. Between two non-canonical spellings the later key in alphabetical order
/// wins (`ldv` over `dev`).
fn canonical_environment_keys(document: &mut Table) {
    let Some(Value::Table(environments)) = document.get_mut("environments") else {
        return;
    };
    let renames: Vec<(String, &str)> = environments
        .keys()
        .filter_map(|key| {
            Environment::built_in(key)
                .filter(|e| e.as_str() != key)
                .map(|e| (key.clone(), e.as_str()))
        })
        .collect();
    for (from, to) in renames {
        if let Some(value) = environments.remove(&from) {
            match (environments.get_mut(to), value) {
                (Some(Value::Table(existing)), Value::Table(incoming)) => {
                    deep_merge(existing, incoming);
                }
                (_, value) => {
                    environments.insert(to.to_owned(), value);
                }
            }
        }
    }
}

/// Merges `overlay` into `base`: tables merge key by key, everything else is replaced.
pub fn deep_merge(base: &mut Table, overlay: Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(existing)), Value::Table(incoming)) => {
                deep_merge(existing, incoming);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> (String, Table) {
        ("test.toml".into(), text.parse().unwrap())
    }

    #[test]
    fn defaults_reproduce_the_python_environment_contract() {
        let config = Config::default();
        assert_eq!(config.core.environment_var, "DBT_DEPLOY_TARGET");
        let cbt = config.environment(Environment::CBT);
        assert_eq!(cbt.vault_url, "https://vault.ops.example.com:8200");
        assert_eq!(cbt.role_id_vars[1], "KEYVAULT_ROLE_ID_LDV_ABT");
        let prd = config.environment(Environment::PRD);
        assert_eq!(prd.ca_certificate, "root_intermediate_prod.cert");
        assert_eq!(config.secrets.precedence[0], "vault:vdp/airflow");
    }

    #[test]
    fn partial_overrides_keep_the_other_defaults() {
        let config = Config::from_documents(vec![doc(
            "[environments.PRD]\nvault_url = \"https://other:8200\"\n[secrets]\nvault_required = true\n",
        )])
        .unwrap();
        let prd = config.environment(Environment::PRD);
        assert_eq!(prd.vault_url, "https://other:8200");
        assert_eq!(prd.ca_certificate, "root_intermediate_prod.cert");
        assert!(config.secrets.vault_required);
        assert_eq!(config.secrets.vault_mount, "kv-other");
    }

    #[test]
    fn typos_are_errors() {
        let error = Config::from_documents(vec![doc("[core]\ndefault_enviroment = \"PRD\"\n")])
            .unwrap_err()
            .to_string();
        assert!(error.contains("default_enviroment"), "{error}");
    }

    #[test]
    fn module_sections_default_and_unknown_sections_are_listed() {
        #[derive(Debug, Default, PartialEq, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        struct Demo {
            flag: bool,
        }
        let config =
            Config::from_documents(vec![doc("[demo]\nflag = true\n[typo]\nx = 1\n")]).unwrap();
        assert_eq!(config.section::<Demo>("demo").unwrap(), Demo { flag: true });
        assert_eq!(config.section::<Demo>("absent").unwrap(), Demo::default());
        assert_eq!(config.unknown_sections(&["demo"]), ["typo"]);
    }

    #[test]
    fn later_files_win() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.toml");
        let b = dir.path().join("b.toml");
        std::fs::write(&a, "[core]\ndefault_environment = \"ABT\"\n").unwrap();
        std::fs::write(&b, "[core]\ndefault_environment = \"PRD\"\n").unwrap();
        let config = Config::load(&[
            (a, false),
            (b, false),
            (dir.path().join("missing.toml"), false),
        ])
        .unwrap();
        assert_eq!(config.core.default_environment, Environment::PRD);
        assert!(Config::load(&[(dir.path().join("missing.toml"), true)]).is_err());
    }

    /// The dbt workshop: DBT_DEPLOY_TARGET=dev|val|prd, no Vault, no TLS. `dev`, `val` and `prd` are the aliases of
    /// `LDV`, `ABT` and `PRD`, so their sections override those environments' defaults key by key.
    #[test]
    fn environments_are_the_configured_sections() {
        let config = Config::from_documents(vec![doc("[core]\ndefault_environment = \"dev\"\n\
             [environments.dev]\nvault_url = \"\"\nca_certificate = \"\"\n\
             [environments.val]\nvault_url = \"\"\n\
             [environments.prd]\nvault_url = \"\"\n\
             [environments.qa-1]\nvault_url = \"\"\n")])
        .unwrap();
        let dev = config.environment_named("DEV").unwrap();
        assert_eq!(dev, Environment::LDV);
        assert_eq!(config.core.default_environment, dev);
        assert_eq!(config.environment(dev).ca_certificate, "");
        assert_eq!(config.environment(dev).vault_url, "");
        // `val` is ABT: the section overrides the non-production defaults key by key.
        let val = config.environment_named("val").unwrap();
        assert_eq!(val, Environment::ABT);
        assert_eq!(config.environment(val).vault_url, "");
        assert_eq!(
            config.environment(val).role_id_vars[1],
            "KEYVAULT_ROLE_ID_LDV_ABT"
        );
        // `prd` is PRD: the section overrides the production defaults key by key.
        let prd = config.environment_named("prd").unwrap();
        assert_eq!(prd, Environment::PRD);
        assert_eq!(config.environment(prd).vault_url, "");
        assert_eq!(
            config.environment(prd).ca_certificate,
            "root_intermediate_prod.cert"
        );
        // A genuinely custom environment is kept as written and starts from the non-production defaults.
        let qa = config.environment_named("QA-1").unwrap();
        assert_eq!(qa.as_str(), "qa-1", "spelled as configured");
        assert!(!qa.is_built_in());
        let error = config.environment_named("tst").unwrap_err().to_string();
        assert!(
            error.contains("'tst'") && error.contains("(configured: LDV, ABT, CBT, PRD, qa-1;"),
            "no dev/val/prd duplicates: {error}"
        );
        // Without sections only the four built-in environments exist, and `dev` is one of them.
        assert_eq!(
            Config::default().environment_named("dev").unwrap(),
            Environment::LDV
        );
        let error = Config::default()
            .environment_named("xyz")
            .unwrap_err()
            .to_string();
        assert!(error.contains("LDV, ABT, CBT, PRD"), "{error}");
        let error = Config::from_documents(vec![doc("[core]\ndefault_environment = \"tst\"\n")])
            .unwrap_err()
            .to_string();
        assert!(error.contains("default_environment"), "{error}");
    }

    /// An alias section and the canonical one are the same section: merged key by key, the alias over the canonical
    /// one, exactly as `prd` over `PRD` always was.
    #[test]
    fn alias_and_canonical_sections_merge_and_the_alias_wins_conflicts() {
        let config = Config::from_documents(vec![doc(
            "[environments.LDV]\nvault_url = \"https://canonical:8200\"\nca_certificate = \"canonical.cert\"\n\
             [environments.dev]\nvault_url = \"https://alias:8200\"\n\
             [environments.PRD]\nvault_url = \"https://canonical-prd:8200\"\n\
             [environments.prd]\nvault_url = \"https://lower-prd:8200\"\n",
        )])
        .unwrap();
        let ldv = config.environment(Environment::LDV);
        assert_eq!(ldv.vault_url, "https://alias:8200", "the alias wins");
        assert_eq!(ldv.ca_certificate, "canonical.cert", "keys of both survive");
        assert_eq!(
            config.environment(Environment::PRD).vault_url,
            "https://lower-prd:8200",
            "`prd` over `PRD`, as before aliases existed"
        );
        let error = config.environment_named("xyz").unwrap_err().to_string();
        assert!(
            error.contains("(configured: LDV, ABT, CBT, PRD;"),
            "{error}"
        );
    }
}
