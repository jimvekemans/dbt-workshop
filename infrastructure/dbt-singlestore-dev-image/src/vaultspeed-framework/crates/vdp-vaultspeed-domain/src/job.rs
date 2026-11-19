use std::fmt;

use serde::{Deserialize, Serialize};

use crate::GenerationId;

/// Stable identity of a `VaultSpeed` job across generations.
///
/// `VaultSpeed` prefixes and suffixes file names with generation numbers, order numbers and
/// timestamps (`629_05_GENERATE_DDL_DV_competent_edw_dv_rdv.sql`). Dropping every purely
/// numeric segment and upper-casing yields an identity that survives regeneration
/// (`GENERATE_DDL_DV_COMPETENT_EDW_DV_RDV`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobName(String);

impl JobName {
    /// Derives the job name from a file stem: purely numeric segments are dropped, the rest is upper-cased.
    pub fn from_file_stem(stem: &str) -> Self {
        let name = stem
            .split('_')
            .filter(|segment| !segment.is_empty() && !is_numeric(segment))
            .map(str::to_ascii_uppercase)
            .collect::<Vec<_>>()
            .join("_");
        Self(name)
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `_`-separated words of the name.
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        self.0.split('_')
    }
}

impl fmt::Display for JobName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Execution order encoded in a `VaultSpeed` file name prefix.
///
/// Up to three leading numeric segments of at most four digits are read
/// (`629_05_...` or `302_001_...`), the segment equal to the generation number is
/// removed, and the rest is compared numerically: `[5] < [10]`, unlike the string
/// comparison `"10" < "5"`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderKey(Vec<u32>);

impl OrderKey {
    const MAX_SEGMENTS: usize = 3;
    const MAX_DIGITS: usize = 4;

    /// Creates a key from its numeric parts.
    pub fn new(parts: Vec<u32>) -> Self {
        Self(parts)
    }

    /// Reads up to three leading numeric segments of `stem` (at most four digits each, and only when another segment follows);
    /// the segment equal to `generation` is removed.
    pub fn from_file_stem(stem: &str, generation: Option<GenerationId>) -> Self {
        let mut segments = stem.split('_').peekable();
        let mut parts = Vec::new();
        while parts.len() < Self::MAX_SEGMENTS {
            match segments.next() {
                // A numeric segment only counts when it is followed by another segment.
                Some(segment)
                    if segment.len() <= Self::MAX_DIGITS
                        && is_numeric(segment)
                        && segments.peek().is_some() =>
                {
                    parts.push(segment.parse().unwrap_or(u32::MAX));
                }
                _ => break,
            }
        }
        if let Some(generation) = generation
            && let Some(index) = parts.iter().position(|p| *p == generation.get())
        {
            parts.remove(index);
        }
        Self(parts)
    }

    /// The first part, if any.
    pub fn first(&self) -> Option<u32> {
        self.0.first().copied()
    }

    /// All parts, in order.
    pub fn parts(&self) -> &[u32] {
        &self.0
    }
}

impl fmt::Display for OrderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self
            .0
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("_");
        f.write_str(&text)
    }
}

fn is_numeric(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_name_drops_numbers_and_uppercases() {
        assert_eq!(
            JobName::from_file_stem("629_05_GENERATE_DDL_DV_competent_edw_dv_rdv").as_str(),
            "GENERATE_DDL_DV_COMPETENT_EDW_DV_RDV"
        );
        assert_eq!(
            JobName::from_file_stem("629_GENERATE_PROC_edw_proc_20250114_142745").as_str(),
            "GENERATE_PROC_EDW_PROC"
        );
        assert_eq!(
            JobName::from_file_stem("AMI_PRE_MODEL_INCR").as_str(),
            "AMI_PRE_MODEL_INCR"
        );
    }

    #[test]
    fn order_key_removes_generation_number() {
        let generation = Some(GenerationId::new(629));
        assert_eq!(
            OrderKey::from_file_stem("629_05_GENERATE_DDL_DV", generation).parts(),
            &[5]
        );
        assert_eq!(
            OrderKey::from_file_stem("629_GENERATE_PROC_edw_proc_20250114", generation),
            OrderKey::default()
        );
    }

    #[test]
    fn order_key_keeps_unrelated_numbers() {
        let key = OrderKey::from_file_stem("302_001_set_fmc_mtd", Some(GenerationId::new(630)));
        assert_eq!(key.parts(), &[302, 1]);
    }

    #[test]
    fn order_key_is_numeric_not_lexical() {
        let five = OrderKey::from_file_stem("1_5_X", Some(GenerationId::new(1)));
        let ten = OrderKey::from_file_stem("1_10_X", Some(GenerationId::new(1)));
        assert!(five < ten);
    }

    #[test]
    fn generation_number_inside_another_segment_is_not_removed() {
        // The Python implementation used `str.replace("30", "")` which mangled `302` into `2`.
        let key = OrderKey::from_file_stem("302_001_x", Some(GenerationId::new(30)));
        assert_eq!(key.parts(), &[302, 1]);
    }
}
