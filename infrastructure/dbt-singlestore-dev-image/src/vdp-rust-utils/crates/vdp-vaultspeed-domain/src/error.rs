use thiserror::Error;

/// Errors raised while turning external text into domain values.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    #[error(
        "`{0}` is not a VaultSpeed generation name (expected `<number>_<TYPE>`, e.g. `1134_FMC`)"
    )]
    InvalidGenerationName(String),

    #[error("unsupported generation type `{kind}` in `{name}` (expected DDL, ETL, FMC or COMP)")]
    UnsupportedGenerationKind { name: String, kind: String },

    #[error("invalid fingerprint `{0}` (expected 64 hexadecimal characters)")]
    InvalidFingerprint(String),

    #[error("unknown {what} `{value}`")]
    UnknownVariant { what: &'static str, value: String },
}
