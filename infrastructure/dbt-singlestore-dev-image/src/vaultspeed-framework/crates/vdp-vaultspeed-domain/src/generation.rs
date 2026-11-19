use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::DomainError;

/// The numeric part of a `VaultSpeed` generation (`1134` in `1134_FMC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GenerationId(u32);

impl GenerationId {
    /// Wraps the number.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// The number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for GenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What a generation zip contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GenerationKind {
    /// `NNN_DDL`: full data definition.
    Ddl,
    /// `NNN_GENERATE_ETL`: load procedures.
    Etl,
    /// `NNN_FMC`: Airflow DAGs and their metadata.
    Fmc,
    /// `NNN_COMP`: delta release with `DDL/` and `ETL/` sub folders.
    Comp,
}

impl GenerationKind {
    fn from_token(token: &str) -> Option<Self> {
        match token.to_ascii_uppercase().as_str() {
            "DDL" => Some(Self::Ddl),
            "ETL" => Some(Self::Etl),
            "FMC" => Some(Self::Fmc),
            "COMP" => Some(Self::Comp),
            _ => None,
        }
    }

    /// The token as it appears in generation names (`DDL`, `ETL`, `FMC`, `COMP`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ddl => "DDL",
            Self::Etl => "ETL",
            Self::Fmc => "FMC",
            Self::Comp => "COMP",
        }
    }
}

impl fmt::Display for GenerationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parsed generation name such as `830_GENERATE_ETL`.
///
/// The number is parsed as an integer, so `113` never matches `1134_FMC.zip` and
/// generations sort numerically instead of lexically.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GenerationName {
    id: GenerationId,
    kind: GenerationKind,
    label: String,
}

impl GenerationName {
    /// The numeric part of the name.
    pub const fn id(&self) -> GenerationId {
        self.id
    }

    /// What the generation contains.
    pub const fn kind(&self) -> GenerationKind {
        self.kind
    }

    /// The name exactly as `VaultSpeed` produced it, without the `.zip` extension.
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl fmt::Display for GenerationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl FromStr for GenerationName {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let label = s.trim();
        let invalid = || DomainError::InvalidGenerationName(s.to_owned());

        let (number, rest) = label.split_once('_').ok_or_else(invalid)?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) || rest.is_empty() {
            return Err(invalid());
        }
        let id = number.parse::<u32>().map_err(|_| invalid())?;

        // `830_GENERATE_ETL` -> `ETL`; `1134_FMC (1)` -> `FMC` (browser download suffixes).
        let token = rest
            .rsplit('_')
            .next()
            .and_then(|last| last.split_whitespace().next())
            .ok_or_else(invalid)?;
        let kind = GenerationKind::from_token(token).ok_or_else(|| {
            DomainError::UnsupportedGenerationKind {
                name: label.to_owned(),
                kind: token.to_owned(),
            }
        })?;

        Ok(Self {
            id: GenerationId(id),
            kind,
            label: label.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> GenerationName {
        s.parse().unwrap()
    }

    #[test]
    fn parses_all_generation_flavours() {
        assert_eq!(parse("1134_FMC").kind(), GenerationKind::Fmc);
        assert_eq!(parse("830_GENERATE_ETL").kind(), GenerationKind::Etl);
        assert_eq!(parse("515_DDL").kind(), GenerationKind::Ddl);
        assert_eq!(parse("463_COMP").kind(), GenerationKind::Comp);
        assert_eq!(parse("1134_FMC").id(), GenerationId::new(1134));
    }

    #[test]
    fn tolerates_download_suffixes() {
        assert_eq!(parse("700_FMC (1)").kind(), GenerationKind::Fmc);
    }

    #[test]
    fn rejects_malformed_names() {
        assert!("FMC".parse::<GenerationName>().is_err());
        assert!("_FMC".parse::<GenerationName>().is_err());
        assert!("12a_FMC".parse::<GenerationName>().is_err());
        assert!(matches!(
            "12_DOC".parse::<GenerationName>(),
            Err(DomainError::UnsupportedGenerationKind { .. })
        ));
    }

    #[test]
    fn ids_sort_numerically() {
        assert!(parse("99_DDL").id() < parse("113_DDL").id());
    }
}
