use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::config::ConfigError;

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct LegacyConfig {
    #[serde(default)]
    fmc_sources: Vec<LegacySource>,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
struct LegacySource {
    source_name: String,
}

/// Reads source names from the Python framework's `config/vaultspeed.json`.
/// A missing file yields no sources.
pub fn load_fmc_sources(path: &Path) -> Result<Vec<String>, ConfigError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_owned(),
        source,
    })?;
    let config: LegacyConfig =
        serde_json::from_str(&text).map_err(|source| ConfigError::FmcSources {
            path: path.to_owned(),
            source,
        })?;
    Ok(config
        .fmc_sources
        .into_iter()
        .map(|s| s.source_name)
        .collect())
}
