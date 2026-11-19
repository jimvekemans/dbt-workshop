use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vdp_vaultspeed_domain::Environment;

use crate::config::ConfigError;

/// `[vaultspeed]` in `vdp.toml`.
///
/// Every value has a default reproducing the Python framework (`python/deploy.py`); deviations are opt-in or
/// documented on the field. Vault (URL, AppRole, CA, mount) comes from the shared `[secrets]`/`[environments]`
/// settings like every other vdp module; only the secret precedence (which Vault paths) is VaultSpeed's own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct VaultspeedSettings {
    /// The vaultspeed-automation-framework checkout (relative to the current directory).
    pub project_dir: PathBuf,
    /// Secret sources in precedence order, as `[secrets].precedence` (`{env}` = the lower-case environment, `ldv`;
    /// `{env_short}` = its short name, `dev`).
    pub secrets_precedence: Vec<String>,
    pub paths: PathSettings,
    pub planning: PlanningSettings,
    pub failure: FailureSettings,
    pub snapshot: SnapshotSettings,
    pub ledger: LedgerSettings,
    pub history: HistorySettings,
    pub dags: DagSettings,
    /// SingleStore per environment. Keys given for an environment override that environment's own defaults.
    #[serde(deserialize_with = "environments_over_defaults")]
    pub environments: BTreeMap<Environment, EnvironmentSettings>,
}

/// `[vaultspeed.environments.<ENV>]` keys, each laid over the defaults of that environment (so a CBT section that
/// only sets `database` keeps CBT's certificate, not LDV's). Built-in environments are always present; custom
/// environments configured in TOML are preserved and ordered after the built-ins.
///
/// Names go through [`Environment::named`], as everywhere else: `dev`, `val` and `prd` are the aliases of `LDV`, `ABT`
/// and `PRD`, and several spellings of one environment (`[environments.LDV]` and `[environments.dev]`) are merged key
/// by key, as in the shared `[environments]` sections of `vdp-core`: the canonical spelling (`LDV`) is applied first and
/// every other spelling after it, so an alias wins a conflict whatever its case (`DEV`, `Dev` and `dev` all win over
/// `LDV`). Between two aliases the later key in alphabetical order wins (`dev` over `DEV`).
fn environments_over_defaults<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<Environment, EnvironmentSettings>, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Partial {
        singlestore_ca_certificate: Option<String>,
        database: Option<String>,
        singlestore_port: Option<u16>,
        dataset_env: Option<String>,
    }
    let given = BTreeMap::<String, Partial>::deserialize(deserializer)?;
    let mut environments: BTreeMap<Environment, EnvironmentSettings> = Environment::ALL
        .into_iter()
        .map(|env| (env, EnvironmentSettings::defaults_for(env)))
        .collect();
    let mut sections = Vec::with_capacity(given.len());
    for (name, partial) in given {
        let env = Environment::named(&name).map_err(serde::de::Error::custom)?;
        sections.push((env, name, partial));
    }
    // The canonical spelling first, every other spelling after it (in key order), so an alias wins a conflict however
    // it is capitalised; the sort is stable, so key order is kept within each group.
    sections.sort_by_key(|(env, name, _)| name != env.as_str());
    for (env, _, partial) in sections {
        let settings = environments
            .entry(env)
            .or_insert_with(|| EnvironmentSettings::defaults_for(env));
        if let Some(v) = partial.singlestore_ca_certificate {
            settings.singlestore_ca_certificate = v;
        }
        if let Some(v) = partial.database {
            settings.database = v;
        }
        if let Some(v) = partial.singlestore_port {
            settings.singlestore_port = v;
        }
        if let Some(v) = partial.dataset_env {
            settings.dataset_env = v;
        }
    }
    Ok(environments)
}

impl Default for VaultspeedSettings {
    fn default() -> Self {
        Self {
            project_dir: PathBuf::from("."),
            secrets_precedence: vec![
                "vault:vdp/airflow".into(),
                "dotenv".into(),
                "vault:vdp/{env}/vaultspeed-automation-framework".into(),
                "env".into(),
            ],
            paths: PathSettings::default(),
            planning: PlanningSettings::default(),
            failure: FailureSettings::default(),
            snapshot: SnapshotSettings::default(),
            ledger: LedgerSettings::default(),
            history: HistorySettings::default(),
            dags: DagSettings::default(),
            environments: Environment::ALL
                .into_iter()
                .map(|env| (env, EnvironmentSettings::defaults_for(env)))
                .collect(),
        }
    }
}

impl VaultspeedSettings {
    /// The name of the config section these settings are read from (`[vaultspeed]`).
    pub const SECTION: &'static str = "vaultspeed";

    /// The settings of `environment` (its defaults when the configuration has no section for it).
    pub fn environment(&self, environment: Environment) -> EnvironmentSettings {
        self.environments
            .get(&environment)
            .cloned()
            .unwrap_or_else(|| EnvironmentSettings::defaults_for(environment))
    }

    /// Rejects values the planner cannot work with.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] when `planning.rdv_order_numbers` is empty or a `dags.default_args` entry has an empty key.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.planning.rdv_order_numbers.is_empty() {
            return Err(ConfigError::Invalid(
                "vaultspeed.planning.rdv_order_numbers must not be empty".into(),
            ));
        }
        if let Some(arg) = self
            .dags
            .default_args
            .iter()
            .find(|a| a.key.trim().is_empty())
        {
            return Err(ConfigError::Invalid(format!(
                "vaultspeed.dags.default_args has an empty key ({:?})",
                arg.value
            )));
        }
        Ok(())
    }
}

/// Where the framework keeps its directories and files, relative to the project directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PathSettings {
    /// The `vaultspeed/` directory (staging, deployed, airflow, logs, ledger).
    pub vaultspeed_dir: PathBuf,
    /// The Python framework's source list (`FMC_SOURCES`), reused as-is.
    pub fmc_sources_file: PathBuf,
    /// Local developer secrets; ignored when absent.
    pub dotenv_file: PathBuf,
    /// Directory holding the CA certificates (Vault and SingleStore).
    pub assets_dir: PathBuf,
}

impl Default for PathSettings {
    fn default() -> Self {
        Self {
            vaultspeed_dir: "vaultspeed".into(),
            fmc_sources_file: "config/vaultspeed.json".into(),
            dotenv_file: ".devcontainer/.env".into(),
            assets_dir: "assets".into(),
        }
    }
}

/// Whether a DDL `DROP` that no `CREATE` follows is skipped or executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StandaloneDropsSetting {
    Skip,
    Execute,
}

/// The `[vaultspeed.planning]` settings: exclusions, phase ordering and the safeguards the planner applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[allow(clippy::struct_excessive_bools)]
pub struct PlanningSettings {
    /// Jobs whose name contains one of these are never executed.
    pub excluded_job_substrings: Vec<String>,
    /// Files whose path (with a leading `/`) contains one of these are never executed.
    pub excluded_path_substrings: Vec<String>,
    pub standalone_drops: StandaloneDropsSetting,
    /// DDL order numbers forming the raw-vault phase, which runs last.
    pub rdv_order_numbers: Vec<u32>,
    /// DDL order numbers whose existing tables are never dropped and re-created with a
    /// changed definition (the planner refuses instead). Empty allows rebuilds.
    pub protected_rebuild_order_numbers: Vec<u32>,
    /// Let an older generation overwrite what a newer generation deployed (rollbacks).
    pub allow_downgrades: bool,
    /// ETL layers that run before all other ETL, in order.
    pub etl_layer_priority: Vec<String>,
    /// Ask `information_schema` before running once-only DDL that the ledger does not know.
    pub catalog_probe: bool,
    /// Accept "already exists" style errors as proof for the matching statement kinds.
    pub classify_errors: bool,
}

impl Default for PlanningSettings {
    fn default() -> Self {
        Self {
            excluded_job_substrings: vec!["DDL_SCHEMAS".into(), "DDL_INI".into(), "DDL_CDC".into()],
            excluded_path_substrings: vec!["/MIGR/".into()],
            standalone_drops: StandaloneDropsSetting::Skip,
            rdv_order_numbers: vec![5],
            protected_rebuild_order_numbers: vec![5],
            allow_downgrades: false,
            etl_layer_priority: vec!["DELTA_FIREWALL_LAYER".into(), "BUSINESS_VAULT_LAYER".into()],
            catalog_probe: true,
            classify_errors: true,
        }
    }
}

/// What to do when a group of an artifact kind fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailurePolicySetting {
    Continue,
    HaltSource,
    HaltRun,
}

/// The `[vaultspeed.failure]` settings: the failure policy per artifact kind and what happens to dependents and archives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FailureSettings {
    /// PML failures stop the run: generated objects depend on PML objects.
    pub pml: FailurePolicySetting,
    pub ddl: FailurePolicySetting,
    pub etl: FailurePolicySetting,
    /// Skip groups touching an object whose CREATE failed earlier in the same source.
    pub skip_dependents: bool,
    /// Archive a generation zip even when some of it failed (Python behaviour). When
    /// false the zip stays staged and the next pipeline retries only what did not run.
    pub archive_on_failure: bool,
}

impl Default for FailureSettings {
    fn default() -> Self {
        Self {
            pml: FailurePolicySetting::HaltRun,
            ddl: FailurePolicySetting::Continue,
            etl: FailurePolicySetting::Continue,
            skip_dependents: true,
            archive_on_failure: false,
        }
    }
}

/// How a `deployed/` mirror file is maintained: appended to, overwritten, or not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotSetting {
    Append,
    Overwrite,
    Disabled,
}

/// How the human-readable `vaultspeed/deployed` mirror is maintained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SnapshotSettings {
    pub ddl: SnapshotSetting,
    pub etl: SnapshotSetting,
    pub pml: SnapshotSetting,
}

impl Default for SnapshotSettings {
    fn default() -> Self {
        Self {
            ddl: SnapshotSetting::Append,
            etl: SnapshotSetting::Overwrite,
            pml: SnapshotSetting::Overwrite,
        }
    }
}

/// Where the ledger is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LedgerBackend {
    /// Table in the environment's database (source of truth) + JSONL mirror in git.
    Database,
    /// JSONL file only (offline use, tests).
    Jsonl,
}

/// The `[vaultspeed.ledger]` settings: the backend and, for the database, the schema and table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LedgerSettings {
    pub backend: LedgerBackend,
    pub schema: String,
    pub table: String,
    /// Mirror database entries to `vaultspeed/ledger/<ENV>.jsonl`.
    pub mirror_jsonl: bool,
}

impl Default for LedgerSettings {
    fn default() -> Self {
        Self {
            backend: LedgerBackend::Database,
            schema: "edw_meta".into(),
            // The table keeps the name the framework created it with.
            table: "vsaf_ledger".into(),
            mirror_jsonl: true,
        }
    }
}

/// `[vaultspeed.history]`: the local object history, a record of every statement of every run linked to
/// the database object it changed. It is an audit and analysis aid only; the ledger decides what runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HistorySettings {
    pub enabled: bool,
    /// The history database file, relative to the framework checkout.
    pub path: PathBuf,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            path: PathBuf::from(".vdp/vaultspeed-history.duckdb"),
        }
    }
}

/// One `default_args` entry; an array keeps the insertion order explicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultArg {
    pub key: String,
    pub value: serde_json::Value,
}

/// The `[vaultspeed.dags]` settings: how DAGs are altered and where they are published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DagSettings {
    pub default_args: Vec<DefaultArg>,
    pub mapping_outlet_prefixes: Vec<String>,
    /// Publish DAGs found in COMP zips (the Python framework only published FMC zips).
    pub publish_comp_dags: bool,
    pub remote_dags_dir: String,
    pub remote_meta_dir: String,
    /// SSH user on the Airflow host (the `username` secret overrides it, as for `vdp airflow`).
    pub ssh_user: String,
    pub ssh_port: u16,
    /// `known_hosts` used for strict host key checking; the user's default when unset.
    pub known_hosts: Option<PathBuf>,
}

impl Default for DagSettings {
    fn default() -> Self {
        let emails = [
            "person1@example.com",
            "person2@example.com",
            "person3@example.com",
            "person4@example.com",
            "person5@example.com",
            "person6@example.com",
            "person7@example.com",
            "person8@example.com",
        ];
        let arg = |key: &str, value: serde_json::Value| DefaultArg {
            key: key.into(),
            value,
        };
        Self {
            default_args: vec![
                arg("email", serde_json::json!(emails)),
                arg("email_on_failure", serde_json::Value::Bool(true)),
                arg("email_on_retry", serde_json::Value::Bool(false)),
                arg("pool", serde_json::Value::String("S2_dv_pool".into())),
            ],
            mapping_outlet_prefixes: ["hub", "lnk", "lnd", "sat", "lks", "lds", "ref"]
                .map(String::from)
                .to_vec(),
            publish_comp_dags: false,
            remote_dags_dir: "/opt/apache_airflow/dags".into(),
            remote_meta_dir: "/opt/apache_airflow/vaultspeed_mtd".into(),
            ssh_user: "airflow".into(),
            ssh_port: 22,
            known_hosts: None,
        }
    }
}

/// The SingleStore database of one environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EnvironmentSettings {
    /// CA for the SingleStore TLS certificate, in `paths.assets_dir`; empty: no TLS.
    pub singlestore_ca_certificate: String,
    pub database: String,
    pub singlestore_port: u16,
    /// Token for `vdp://<token>/...` dataset URIs; empty: derived from the host (`rel1...`).
    pub dataset_env: String,
}

impl Default for EnvironmentSettings {
    fn default() -> Self {
        Self::defaults_for(Environment::LDV)
    }
}

impl EnvironmentSettings {
    /// CBT and PRD use the production CA (as the framework did), every other environment the non-production CA.
    pub fn defaults_for(environment: Environment) -> Self {
        let certificate = if environment == Environment::CBT || environment == Environment::PRD {
            "root_intermediate_prod.cert"
        } else {
            "root_intermediate_nonprod.cert"
        };
        Self {
            singlestore_ca_certificate: certificate.into(),
            database: "edw_dv_rdv".into(),
            singlestore_port: 3306,
            dataset_env: String::new(),
        }
    }

    /// The dataset token for `vdp://<token>/...` URIs, `None` when it is derived from the host.
    pub fn dataset_env(&self) -> Option<&str> {
        Some(self.dataset_env.as_str()).filter(|s| !s.is_empty())
    }

    /// The CA certificate file for the SingleStore connection, `None` when there is no TLS.
    pub fn singlestore_ca_certificate(&self) -> Option<&str> {
        Some(self.singlestore_ca_certificate.as_str()).filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<VaultspeedSettings, toml::de::Error> {
        toml::from_str(text)
    }

    #[test]
    fn empty_section_gives_python_parity_defaults() {
        let settings = parse("").unwrap();
        assert_eq!(settings, VaultspeedSettings::default());
        assert_eq!(
            settings.planning.excluded_job_substrings,
            ["DDL_SCHEMAS", "DDL_INI", "DDL_CDC"]
        );
        assert_eq!(settings.dags.default_args.len(), 4);
        assert_eq!(
            settings
                .environment(Environment::PRD)
                .singlestore_ca_certificate,
            "root_intermediate_prod.cert"
        );
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn history_defaults_and_overrides() {
        let defaults = parse("").unwrap().history;
        assert!(defaults.enabled);
        assert_eq!(
            defaults.path,
            PathBuf::from(".vdp/vaultspeed-history.duckdb")
        );
        let off = parse("[history]\nenabled = false\npath = \"h.duckdb\"\n").unwrap();
        assert!(!off.history.enabled);
        assert_eq!(off.history.path, PathBuf::from("h.duckdb"));
        assert!(parse("[history]\ntypo = 1\n").is_err());
    }

    #[test]
    fn overrides_and_unknown_keys() {
        assert!(
            parse("[environments.CBT]\nsingletore_port_typo = 1\n")
                .is_err_and(|e| e.to_string().contains("singletore_port_typo"))
        );
        let settings = parse(
            "[failure]\npml = \"continue\"\n[[dags.default_args]]\nkey = \"retries\"\nvalue = 5\n\
             [environments.LDV]\ndatabase = \"other\"\n[environments.CBT]\ndatabase = \"cbt_db\"\n",
        )
        .unwrap();
        assert_eq!(settings.failure.pml, FailurePolicySetting::Continue);
        assert_eq!(
            settings.dags.default_args,
            vec![DefaultArg {
                key: "retries".into(),
                value: serde_json::json!(5)
            }]
        );
        let ldv = settings.environment(Environment::LDV);
        assert_eq!(ldv.database, "other");
        assert_eq!(ldv.singlestore_port, 3306);
        // A partial section keeps the rest of that environment's own defaults.
        let cbt = settings.environment(Environment::CBT);
        assert_eq!(cbt.database, "cbt_db");
        assert_eq!(
            cbt.singlestore_ca_certificate,
            "root_intermediate_prod.cert"
        );
        assert!(parse("[planning]\ntypo = 1\n").is_err());
        assert!(
            parse("[planning]\nrdv_order_numbers = []\n")
                .unwrap()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn custom_environments_are_merged_and_ordered_after_builtins() {
        let settings = parse(
            "[environments.sandbox]\ndatabase = \"sandbox_db\"\nsinglestore_port = 3307\n\
             [environments.qa-2]\nsinglestore_ca_certificate = \"\"\ndatabase = \"qa2_db\"\ndataset_env = \"qa\"\n\
             [environments.qa-1]\ndatabase = \"qa_db\"\n",
        )
        .unwrap();

        let keys: Vec<&str> = settings.environments.keys().map(|e| e.as_str()).collect();
        assert_eq!(
            keys,
            ["LDV", "ABT", "CBT", "PRD", "qa-1", "qa-2", "sandbox"]
        );

        let qa2 = settings.environment(Environment::named("qa-2").unwrap());
        assert_eq!(qa2.database, "qa2_db");
        assert_eq!(qa2.singlestore_ca_certificate, "");
        assert_eq!(qa2.singlestore_port, 3306);
        assert_eq!(qa2.dataset_env, "qa");

        let sandbox = settings.environment(Environment::named("sandbox").unwrap());
        assert_eq!(sandbox.database, "sandbox_db");
        assert_eq!(sandbox.singlestore_port, 3307);
        assert_eq!(
            sandbox.singlestore_ca_certificate,
            "root_intermediate_nonprod.cert"
        );

        let qa = settings.environment(Environment::named("qa-1").unwrap());
        assert_eq!(qa.database, "qa_db");

        // Built-ins remain defaulted.
        assert_eq!(
            settings
                .environment(Environment::PRD)
                .singlestore_ca_certificate,
            "root_intermediate_prod.cert"
        );
        // Invalid environment names are still rejected.
        assert!(parse("[environments.\"bad name\"]\ndatabase = \"x\"\n").is_err());
    }

    /// The canonical section is applied first and every alias after it, whatever the spelling or case of the alias:
    /// `DEV`, `Dev` and `dev` all win a conflict over `LDV`, as in `vdp-core`. (Plain key order would let `LDV` win
    /// over `DEV` and `Dev`, which sort before it.)
    #[test]
    fn an_alias_section_wins_over_the_canonical_one_whatever_its_case() {
        for alias in ["DEV", "Dev", "dev"] {
            let settings = parse(&format!(
                "[environments.LDV]\ndatabase = \"ldv_db\"\nsinglestore_port = 3307\n\
                 [environments.{alias}]\ndatabase = \"alias_db\"\ndataset_env = \"alias\"\n"
            ))
            .unwrap();
            let ldv = settings.environment(Environment::LDV);
            assert_eq!(
                ldv.database, "alias_db",
                "[environments.{alias}] wins over LDV"
            );
            assert_eq!(ldv.singlestore_port, 3307, "{alias}: keys of both survive");
            assert_eq!(ldv.dataset_env, "alias");
            let keys: Vec<&str> = settings.environments.keys().map(|e| e.as_str()).collect();
            assert_eq!(
                keys,
                ["LDV", "ABT", "CBT", "PRD"],
                "{alias}: nothing custom"
            );
        }
    }

    /// `dev`, `val` and `prd` are aliases of `LDV`, `ABT` and `PRD`: their sections land on the built-ins, over those
    /// environments' own defaults, and merge with a section written under the canonical name.
    #[test]
    fn alias_sections_land_on_the_built_in_environments() {
        let settings = parse(
            "[environments.dev]\ndatabase = \"dev_db\"\ndataset_env = \"dev\"\n\
             [environments.LDV]\ndatabase = \"ldv_db\"\nsinglestore_port = 3307\n\
             [environments.val]\ndatabase = \"val_db\"\n\
             [environments.prd]\nsinglestore_ca_certificate = \"\"\n",
        )
        .unwrap();

        let keys: Vec<&str> = settings.environments.keys().map(|e| e.as_str()).collect();
        assert_eq!(
            keys,
            ["LDV", "ABT", "CBT", "PRD"],
            "nothing custom is added"
        );

        let ldv = settings.environment(Environment::LDV);
        assert_eq!(ldv.database, "dev_db", "the alias wins a conflict");
        assert_eq!(ldv.singlestore_port, 3307, "keys of both sections survive");
        assert_eq!(ldv.dataset_env, "dev");
        assert_eq!(settings.environment(Environment::ABT).database, "val_db");
        let prd = settings.environment(Environment::PRD);
        assert_eq!(prd.singlestore_ca_certificate, "");
        assert_eq!(prd.database, "edw_dv_rdv");
        // The alias resolves to the same settings as the canonical name.
        assert_eq!(
            settings.environment(Environment::named("dev").unwrap()),
            ldv
        );
    }
}
