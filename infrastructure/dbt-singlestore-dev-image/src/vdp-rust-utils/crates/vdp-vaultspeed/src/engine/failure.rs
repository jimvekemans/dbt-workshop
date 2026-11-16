use crate::executor::FailurePolicy;
use vdp_vaultspeed_domain::{ArtifactKind, GenerationKind, SourceKind};

/// Failure policy per artifact kind, resolved per source.
#[derive(Debug, Clone, Copy)]
pub struct FailurePolicies {
    pub pml: FailurePolicy,
    pub ddl: FailurePolicy,
    pub etl: FailurePolicy,
}

impl FailurePolicies {
    pub const fn for_artifact(&self, artifact: ArtifactKind) -> FailurePolicy {
        match artifact {
            ArtifactKind::Pml => self.pml,
            ArtifactKind::Ddl => self.ddl,
            ArtifactKind::Etl => self.etl,
        }
    }

    /// A COMP generation contains DDL and ETL; the stricter policy applies.
    pub fn for_source(&self, source: &SourceKind) -> FailurePolicy {
        match source {
            SourceKind::Pml => self.pml,
            SourceKind::Generation(name) => match name.kind() {
                GenerationKind::Ddl => self.ddl,
                GenerationKind::Etl => self.etl,
                GenerationKind::Comp => stricter(self.ddl, self.etl),
                GenerationKind::Fmc => FailurePolicy::Continue,
            },
        }
    }
}

const fn severity(policy: FailurePolicy) -> u8 {
    match policy {
        FailurePolicy::Continue => 0,
        FailurePolicy::HaltSource => 1,
        FailurePolicy::HaltRun => 2,
    }
}

const fn stricter(a: FailurePolicy, b: FailurePolicy) -> FailurePolicy {
    if severity(a) >= severity(b) { a } else { b }
}
