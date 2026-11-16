use std::fmt::Debug;

use vdp_vaultspeed_domain::{ArtifactKind, Layer, StatementGroup};

/// Orders the groups of one artifact kind into named phases.
pub trait ArtifactPhases: Send + Sync + Debug {
    fn artifact(&self) -> ArtifactKind;
    fn phases(&self, groups: Vec<StatementGroup>) -> Vec<(String, Vec<StatementGroup>)>;
}

/// Orders all groups of a source into named phases.
pub trait PhaseStrategy: Send + Sync + Debug {
    fn phases(&self, groups: Vec<StatementGroup>) -> Vec<(String, Vec<StatementGroup>)>;
}

/// Runs artifact strategies in sequence (e.g. PML, then DDL, then ETL), so a COMP
/// generation's DDL completes before its ETL starts.
#[derive(Debug)]
pub struct SequentialPhasing {
    strategies: Vec<Box<dyn ArtifactPhases>>,
}

impl SequentialPhasing {
    pub fn new(strategies: Vec<Box<dyn ArtifactPhases>>) -> Self {
        Self { strategies }
    }
}

impl PhaseStrategy for SequentialPhasing {
    fn phases(&self, mut groups: Vec<StatementGroup>) -> Vec<(String, Vec<StatementGroup>)> {
        let mut phases = Vec::new();
        for strategy in &self.strategies {
            let (mine, rest): (Vec<_>, Vec<_>) = groups
                .into_iter()
                .partition(|g| g.info.artifact == strategy.artifact());
            groups = rest;
            phases.extend(strategy.phases(mine));
        }
        if !groups.is_empty() {
            phases.push(("unphased".to_owned(), sorted(groups)));
        }
        phases.retain(|(_, groups)| !groups.is_empty());
        phases
    }
}

fn sorted(mut groups: Vec<StatementGroup>) -> Vec<StatementGroup> {
    groups.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    groups
}

/// PML runs as a single phase in file order.
#[derive(Debug, Clone, Copy, Default)]
pub struct PmlPhases;

impl ArtifactPhases for PmlPhases {
    fn artifact(&self) -> ArtifactKind {
        ArtifactKind::Pml
    }

    fn phases(&self, groups: Vec<StatementGroup>) -> Vec<(String, Vec<StatementGroup>)> {
        vec![("pml".to_owned(), sorted(groups))]
    }
}

/// DDL: everything except the raw vault first, then the raw vault (order number 5),
/// because RDV objects depend on staging and metadata objects.
#[derive(Debug, Clone)]
pub struct DdlPhases {
    rdv_order_numbers: Vec<u32>,
}

impl DdlPhases {
    pub const fn new(rdv_order_numbers: Vec<u32>) -> Self {
        Self { rdv_order_numbers }
    }
}

impl ArtifactPhases for DdlPhases {
    fn artifact(&self) -> ArtifactKind {
        ArtifactKind::Ddl
    }

    fn phases(&self, groups: Vec<StatementGroup>) -> Vec<(String, Vec<StatementGroup>)> {
        let (rdv, other): (Vec<_>, Vec<_>) = sorted(groups).into_iter().partition(|g| {
            g.info
                .order
                .first()
                .is_some_and(|n| self.rdv_order_numbers.contains(&n))
        });
        vec![
            ("ddl:non-rdv".to_owned(), other),
            ("ddl:rdv".to_owned(), rdv),
        ]
    }
}

/// ETL: prioritised layers first (delta firewall views, then business vault), then the
/// remaining procedures.
#[derive(Debug, Clone)]
pub struct EtlPhases {
    layer_priority: Vec<Layer>,
}

impl EtlPhases {
    pub const fn new(layer_priority: Vec<Layer>) -> Self {
        Self { layer_priority }
    }
}

impl ArtifactPhases for EtlPhases {
    fn artifact(&self) -> ArtifactKind {
        ArtifactKind::Etl
    }

    fn phases(&self, groups: Vec<StatementGroup>) -> Vec<(String, Vec<StatementGroup>)> {
        let mut remaining = sorted(groups);
        let mut phases = Vec::new();
        for layer in &self.layer_priority {
            let (mine, rest): (Vec<_>, Vec<_>) =
                remaining.into_iter().partition(|g| &g.info.layer == layer);
            remaining = rest;
            phases.push((format!("etl:{}", kebab(layer.as_str())), mine));
        }
        phases.push(("etl:remaining".to_owned(), remaining));
        phases
    }
}

fn kebab(layer: &str) -> String {
    layer
        .trim_end_matches("_LAYER")
        .to_lowercase()
        .replace('_', "-")
}
