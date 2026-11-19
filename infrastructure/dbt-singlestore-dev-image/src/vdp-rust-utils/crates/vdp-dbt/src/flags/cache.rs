//! The installed engine's grammar, generated once per binary and cached.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::flags::{Grammar, generate};

/// Where grammars are cached: `$XDG_CACHE_HOME/vdp`, else `~/.cache/vdp`.
pub fn cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|dir| dir.join("vdp"))
}

/// A key that changes whenever the binary does (path, size and modification time).
fn binary_key(binary: &Path) -> Option<String> {
    let canonical = std::fs::canonicalize(binary).ok()?;
    let meta = std::fs::metadata(&canonical).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let identity = format!("{}|{}|{mtime}", canonical.display(), meta.len());
    Some(format!("{:016x}", fnv1a(identity.as_bytes())))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The grammar of `binary`: from the cache, else generated (and cached when possible), else the embedded one.
pub fn for_binary(binary: &Path) -> Grammar {
    let key = binary_key(binary);
    let cached = key
        .as_ref()
        .zip(cache_dir())
        .map(|(key, dir)| dir.join(format!("dbt-grammar-{key}.json")));
    if let Some(path) = &cached
        && let Ok(text) = std::fs::read_to_string(path)
        && let Ok(grammar) = serde_json::from_str(&text)
    {
        return grammar;
    }
    match generate::from_binary(binary) {
        Ok(grammar) => {
            if let Some(path) = &cached {
                store(path, &grammar);
            }
            grammar
        }
        Err(error) => {
            tracing::debug!(error = %vdp_core::display_chain(&error), "cannot derive the dbt grammar from the engine; using the embedded one");
            Grammar::embedded()
        }
    }
}

fn store(path: &Path, grammar: &Grammar) {
    let Some(dir) = path.parent() else { return };
    let Ok(json) = serde_json::to_string(grammar) else {
        return;
    };
    // Write a sibling and rename, so concurrent wrappers never read half a file.
    let temp = dir.join(format!(".{}.{}", std::process::id(), "tmp"));
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&temp, json))
        .and_then(|()| std::fs::rename(&temp, path));
    if let Err(error) = written {
        tracing::debug!(%error, path = %path.display(), "cannot cache the dbt grammar");
        let _ = std::fs::remove_file(&temp);
    }
}
