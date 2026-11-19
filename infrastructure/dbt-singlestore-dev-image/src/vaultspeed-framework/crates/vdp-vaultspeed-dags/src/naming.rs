use std::fmt::Debug;

/// Decides the published file name of a DAG.
pub trait DagNamer: Send + Sync + Debug {
    /// The file name to publish the DAG `source_name` under.
    fn target_name(&self, source_name: &str) -> String;
}

/// `1134_FL_DAG_refkader_incr_20251023_104445.py` → `FL_DAG_REFKADER_INCR.py`, so each
/// generation replaces the previous version of the same DAG.
#[derive(Debug, Clone, Copy, Default)]
pub struct VaultSpeedDagNamer;

impl DagNamer for VaultSpeedDagNamer {
    fn target_name(&self, source_name: &str) -> String {
        let (stem, extension) = source_name.rsplit_once('.').unwrap_or((source_name, "py"));
        let stem = stem
            .split('_')
            .filter(|s| !s.is_empty() && !s.bytes().all(|b| b.is_ascii_digit()))
            .map(str::to_ascii_uppercase)
            .collect::<Vec<_>>()
            .join("_");
        format!("{stem}.{extension}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_generation_and_timestamp() {
        assert_eq!(
            VaultSpeedDagNamer.target_name("1134_FL_DAG_refkader_incr_20251023_104445.py"),
            "FL_DAG_REFKADER_INCR.py"
        );
    }
}
