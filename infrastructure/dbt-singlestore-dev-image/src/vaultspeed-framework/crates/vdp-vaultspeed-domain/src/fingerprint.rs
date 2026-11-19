use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::DomainError;

/// SHA-256 identity of a piece of (normalised) SQL.
///
/// The fingerprint is the unit of "has this already run?": two statements with the same
/// fingerprint are considered the same change, regardless of whitespace, comments or
/// the generation they arrived in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// Hashes arbitrary bytes. Callers are expected to normalise SQL first.
    pub fn of(bytes: impl AsRef<[u8]>) -> Self {
        Self(Sha256::digest(bytes.as_ref()).into())
    }

    /// Order-sensitive combination of several fingerprints (e.g. all statements of a group).
    pub fn combine<'a>(parts: impl IntoIterator<Item = &'a Self>) -> Self {
        let mut hasher = Sha256::new();
        for part in parts {
            hasher.update(part.0);
        }
        Self(hasher.finalize().into())
    }

    /// The 64 lower-case hex characters of the digest.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// First 12 hex characters, for human-facing output.
    pub fn short(&self) -> String {
        let mut hex = self.to_hex();
        hex.truncate(12);
        hex
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.short())
    }
}

impl FromStr for Fingerprint {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0_u8; 32];
        hex::decode_to_slice(s.trim(), &mut bytes)
            .map_err(|_| DomainError::InvalidFingerprint(s.to_owned()))?;
        Ok(Self(bytes))
    }
}

impl Serialize for Fingerprint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let fp = Fingerprint::of("ALTER TABLE x ADD COLUMN y INT");
        assert_eq!(fp.to_hex().parse::<Fingerprint>(), Ok(fp));
        assert_eq!(fp.short().len(), 12);
    }

    #[test]
    fn combine_is_order_sensitive() {
        let a = Fingerprint::of("a");
        let b = Fingerprint::of("b");
        assert_ne!(
            Fingerprint::combine([&a, &b]),
            Fingerprint::combine([&b, &a])
        );
    }

    #[test]
    fn serde_uses_hex_strings() {
        let fp = Fingerprint::of("x");
        let json = serde_json::to_string(&fp).unwrap();
        assert_eq!(json, format!("\"{}\"", fp.to_hex()));
        assert_eq!(serde_json::from_str::<Fingerprint>(&json).unwrap(), fp);
    }
}
