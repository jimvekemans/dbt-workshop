use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{JobName, RelPath};

/// `VaultSpeed` architecture layer of a job; drives ETL phase ordering.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Layer {
    Extraction,
    Staging,
    Foundation,
    BusinessVault,
    DeltaFirewall,
    FlowManagementControl,
    Other(String),
    NotApplicable,
}

impl Layer {
    const FOUNDATION_PREFIXES: [&'static str; 6] = ["LKS", "SAT", "REF", "LDS", "HUB", "LNK"];
    const FMC_PREFIXES: [&'static str; 6] = ["LCI", "LWT", "BVLCI", "BVLWT", "DVLCI", "FMCH"];

    /// Maps a `VaultSpeed` layer directory name (`FOUNDATION_LAYER`, ...) to a layer.
    pub fn from_dir_name(name: &str) -> Self {
        match name.to_ascii_uppercase().as_str() {
            "EXTRACTION_LAYER" => Self::Extraction,
            "STAGING_LAYER" => Self::Staging,
            "FOUNDATION_LAYER" => Self::Foundation,
            "BUSINESS_VAULT_LAYER" => Self::BusinessVault,
            "DELTA_FIREWALL_LAYER" => Self::DeltaFirewall,
            "FLOW_MANAGEMENT_CONTROL_LAYER" => Self::FlowManagementControl,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Infers the layer from the directory layout (`INCR/FOUNDATION_LAYER/x.sql`) and,
    /// for flat layouts, from the job name prefix.
    pub fn infer(rel_dir: &RelPath, job: &JobName) -> Self {
        if let Some(layer_dir) = rel_dir.components().nth(1) {
            return Self::from_dir_name(layer_dir);
        }
        let tokens: Vec<&str> = job.tokens().collect();
        let first = tokens.first().copied().unwrap_or_default();
        if Self::FOUNDATION_PREFIXES.contains(&first) {
            Self::Foundation
        } else if Self::FMC_PREFIXES.contains(&first) {
            Self::FlowManagementControl
        } else if tokens.contains(&"BV") {
            Self::BusinessVault
        } else if tokens.iter().any(|t| matches!(*t, "TDFV" | "TDFT")) {
            Self::DeltaFirewall
        } else {
            Self::NotApplicable
        }
    }

    /// The layer as `VaultSpeed` spells it (`FOUNDATION_LAYER`, ...); `N/A` for [`Layer::NotApplicable`].
    pub fn as_str(&self) -> &str {
        match self {
            Self::Extraction => "EXTRACTION_LAYER",
            Self::Staging => "STAGING_LAYER",
            Self::Foundation => "FOUNDATION_LAYER",
            Self::BusinessVault => "BUSINESS_VAULT_LAYER",
            Self::DeltaFirewall => "DELTA_FIREWALL_LAYER",
            Self::FlowManagementControl => "FLOW_MANAGEMENT_CONTROL_LAYER",
            Self::Other(name) => name,
            Self::NotApplicable => "N/A",
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_from_directory() {
        let layer = Layer::infer(
            &RelPath::new("INCR/FOUNDATION_LAYER"),
            &JobName::from_file_stem("x"),
        );
        assert_eq!(layer, Layer::Foundation);
    }

    #[test]
    fn layer_from_job_name() {
        let infer = |job| Layer::infer(&RelPath::new(""), &JobName::from_file_stem(job));
        assert_eq!(infer("HUB_AMI_X"), Layer::Foundation);
        assert_eq!(infer("LCI_X"), Layer::FlowManagementControl);
        assert_eq!(infer("GENERATE_DDL_BV_EDW"), Layer::BusinessVault);
        assert_eq!(infer("SRC_X_TDFV_INCR"), Layer::DeltaFirewall);
        assert_eq!(infer("GENERATE_DDL_MTD"), Layer::NotApplicable);
    }
}
