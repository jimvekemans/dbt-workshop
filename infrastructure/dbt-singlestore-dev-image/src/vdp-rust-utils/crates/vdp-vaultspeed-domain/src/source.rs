use std::fmt;

use serde::{Deserialize, Serialize};

use crate::GenerationName;

/// Where a batch of work comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "name", rename_all = "snake_case")]
pub enum SourceKind {
    /// Hand-written pre-model scripts staged in `staging/pre_model_input`.
    Pml,
    /// A `VaultSpeed` generation zip staged in `staging/release`.
    Generation(GenerationName),
}

impl SourceKind {
    pub fn label(&self) -> &str {
        match self {
            Self::Pml => "PML",
            Self::Generation(name) => name.label(),
        }
    }

    /// The default ordering rule: PML before everything, then generations by number.
    pub fn default_priority(&self) -> Priority {
        match self {
            Self::Pml => Priority::HIGHEST,
            Self::Generation(name) => {
                Priority(Priority::GENERATION_BASE + u64::from(name.id().get()))
            }
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Lower value runs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Priority(u64);

impl Priority {
    pub const HIGHEST: Self = Self(0);
    const GENERATION_BASE: u64 = 1_000;

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pml_always_precedes_generations() {
        let pml = SourceKind::Pml.default_priority();
        let first = SourceKind::Generation("1_DDL".parse().unwrap()).default_priority();
        assert!(pml < first);
    }

    #[test]
    fn generations_order_by_number() {
        let older = SourceKind::Generation("999_FMC".parse().unwrap()).default_priority();
        let newer = SourceKind::Generation("1000_DDL".parse().unwrap()).default_priority();
        assert!(older < newer);
    }
}
