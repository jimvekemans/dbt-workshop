use serde::{Deserialize, Serialize};

/// `[airflow]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AirflowSettings {
    /// Secrets with the host (first one set wins; a scheme and `:port` are allowed).
    pub host_secrets: Vec<String>,
    /// Secrets with the SSH user (`vdp/airflow` has `username`).
    pub user_secrets: Vec<String>,
    /// Secrets with the private key (`vdp/airflow` has `private_key`). None: the SSH agent / default keys.
    pub key_secrets: Vec<String>,
    pub port: u16,
    /// known_hosts file; empty: the user's own. Host keys are always checked.
    pub known_hosts: String,
    /// The dbt project checkout on the Airflow host.
    pub remote_project_dir: String,
    /// Where its DAGs are linked for the scheduler.
    pub remote_dags_dir: String,
    pub link_dags: bool,
}

impl Default for AirflowSettings {
    fn default() -> Self {
        Self {
            host_secrets: vec!["AIRFLOW_HOST".into(), "AIRFLOW_URL".into()],
            user_secrets: vec!["username".into(), "AIRFLOW_USER".into()],
            key_secrets: vec!["private_key".into(), "AIRFLOW_PRIVATE_KEY".into()],
            port: 22,
            known_hosts: String::new(),
            remote_project_dir: "/opt/apache_airflow/dbt/dbt-swf".into(),
            remote_dags_dir: "/opt/apache_airflow/dags/dbt".into(),
            link_dags: true,
        }
    }
}

impl AirflowSettings {
    pub const SECTION: &'static str = "airflow";
}
