use vdp_vaultspeed_domain::Environment;

/// Environment facts transformers may embed into a DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagContext {
    pub environment: Environment,
    /// Environment token used in `vdp://` dataset URIs (e.g. `rel1ldv`).
    pub dataset_env: String,
    /// `SingleStore` database (dataset "zone"), e.g. `edw_dv_rdv`.
    pub database: String,
    pub singlestore_host: String,
    pub singlestore_port: u16,
}
